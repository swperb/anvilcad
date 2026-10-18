//! Extrusions with every end condition, two directions, draft, thin walls, caps and
//! flipped cuts, built on both kernels. The behaviour is specified in
//! `docs/re/extrude-revolve.md`.
//!
//! An extrusion is planned first: its regions in the sketch frame and one or two
//! *pieces*, each a run along the sketch normal from the start plane to where its end
//! condition stops it. A piece ends at a set distance (drafted or not), or runs on past a
//! leaning plane and is trimmed by it. The tool is the union of its pieces, hollowed for a
//! capped thin wall, or turned inside out within its length for a flipped cut. The same
//! plan then builds the faceted tool and, when the part is exact, the exact one.

use crate::document::*;
use crate::exact;
use crate::rebuild::BuildState;
use anvil_brep as brep;
use anvil_brep::boolean::{boolean, BoolOp};
use anvil_kernel::build::extrude_drafted;
use anvil_kernel::{v2, Frame, Loop2, Profile, Solid, Vec3};

/// A plane that cuts off the end of a piece: points with `(p - origin) . normal > 0`
/// are removed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Trim {
    origin: Vec3,
    normal: Vec3,
}

/// One run of the extrusion, in the start frame's z: from `z0` to `z1`, the section
/// moved `z * taper` outwards at height `z`, and cut off by `trim` if set.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Piece {
    z0: f64,
    z1: f64,
    taper: f64,
    trim: Option<Trim>,
}

pub(crate) struct Plan {
    /// The start plane (the sketch plane, or offset from it).
    frame: Frame,
    profiles: Vec<Profile>,
    pieces: Vec<Piece>,
    /// A capped thin wall: the outlines filled in, less the hollows from `cap` inside
    /// each end.
    capped: Option<(Vec<Profile>, Vec<Profile>, f64)>,
    flip: bool,
    /// Half the size of a box that holds the part and the profile.
    reach: f64,
}

/// How far one direction goes.
enum Reach {
    Distance(f64),
    Plane(Trim, f64),
}

fn world_points(frame: &Frame, profiles: &[Profile]) -> Vec<Vec3> {
    profiles
        .iter()
        .flat_map(|p| p.loops())
        .flat_map(|l| l.polyline(0.01))
        .map(|p| frame.to_world(p))
        .collect()
}

/// Points inside the profiles, for finding what the extrusion runs into.
fn inner_points(frame: &Frame, profiles: &[Profile]) -> Vec<Vec3> {
    let mut out = Vec::new();
    for p in profiles {
        let outer = p.outer.polyline(0.05);
        let holes: Vec<Vec<anvil_kernel::Vec2>> = p.holes.iter().map(|h| h.polyline(0.05)).collect();
        let (pts, tris) = anvil_kernel::triangulate::triangulate(&outer, &holes);
        for t in tris {
            let c = (pts[t[0]] + pts[t[1]] + pts[t[2]]) * (1.0 / 3.0);
            out.push(frame.to_world(c));
        }
    }
    out
}

impl Document {
    /// The plane of an Up To or Offset From Surface target.
    fn target_plane(&self, t: &Option<EndTarget>, st: &BuildState) -> Result<(Frame, bool, bool), String> {
        match t {
            Some(EndTarget::Plane { plane, reverse_offset, translate }) => {
                Ok((self.resolve_plane(plane, st)?, *reverse_offset, *translate))
            }
            Some(EndTarget::Point(_)) => {
                Err("this end condition needs a plane or flat face, not a point".into())
            }
            None => Err("choose the plane or face the extrusion goes up to".into()),
        }
    }

    pub(crate) fn resolve_point(&self, p: &PointRef, st: &BuildState) -> Result<Vec3, String> {
        match *p {
            PointRef::Model(v) => Ok(v),
            PointRef::SketchPoint { sketch, point } => {
                let frame =
                    st.frames.get(&sketch).ok_or("the sketch holding the point failed or is missing")?;
                let sk = &self.sketch(sketch).ok_or("the sketch holding the point is missing")?.sketch;
                let q = sk.points.get(point).ok_or("the point is no longer in its sketch")?;
                Ok(frame.to_world(q.pos()))
            }
        }
    }

    /// Where one direction ends, measured along `u` from the start plane.
    #[allow(clippy::too_many_arguments)]
    fn reach(
        &self,
        end: EndCondition,
        depth: &Dim,
        target: &Option<EndTarget>,
        start: &Frame,
        u: Vec3,
        boss: bool,
        profiles: &[Profile],
        st: &BuildState,
    ) -> Result<Reach, String> {
        let num = |d: &Dim, what: &str| d.eval(&st.variables).map_err(|e| format!("{what}: {e}"));
        let positive = |d: f64, what: &str| {
            if d > 1e-9 && d.is_finite() {
                Ok(d)
            } else {
                Err(format!("the {what} must be greater than zero"))
            }
        };
        // Up to a plane, leaning or not.
        let to_plane = |origin: Vec3, normal: Vec3| -> Result<Reach, String> {
            let un = u.dot(normal);
            if un.abs() < 1e-9 {
                return Err(
                    "the surface runs along the direction of extrusion, so it is never reached".into()
                );
            }
            let ts: Vec<f64> =
                world_points(start, profiles).iter().map(|p| (origin - *p).dot(normal) / un).collect();
            let (lo, hi) =
                ts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &t| (a.min(t), b.max(t)));
            if lo <= 1e-9 {
                return Err("part of the profile does not reach the surface in this direction".into());
            }
            if hi - lo < 1e-9 {
                return Ok(Reach::Distance(lo));
            }
            Ok(Reach::Plane(Trim { origin, normal: normal * un.signum() }, hi + 1.0))
        };
        match end {
            EndCondition::Blind => Ok(Reach::Distance(positive(num(depth, "depth")?, "depth")?)),
            EndCondition::MidPlane => Ok(Reach::Distance(positive(num(depth, "depth")?, "depth")? / 2.0)),
            EndCondition::ThroughAll | EndCondition::ThroughAllBoth => {
                if st.body.is_empty() {
                    return Err("there is no geometry to extrude through".into());
                }
                if boss {
                    // A boss ends where the existing geometry ends, never past it.
                    let far = st
                        .body
                        .polygons
                        .iter()
                        .flat_map(|p| p.verts.iter())
                        .map(|v| (*v - start.origin).dot(u))
                        .fold(f64::NEG_INFINITY, f64::max);
                    if far <= 1e-9 {
                        return Err("there is no geometry in this direction to extrude up to".into());
                    }
                    Ok(Reach::Distance(far))
                } else {
                    let b = st.body.bounds();
                    Ok(Reach::Distance(b.diagonal() + (b.center() - start.origin).length() + 1.0))
                }
            }
            EndCondition::UpToVertex => {
                let Some(EndTarget::Point(p)) = target else {
                    return Err("choose the vertex or point the extrusion goes up to".into());
                };
                let d = (self.resolve_point(p, st)? - start.origin).dot(u);
                if d <= 1e-9 {
                    return Err("the vertex is on or behind the start of the extrusion".into());
                }
                Ok(Reach::Distance(d))
            }
            EndCondition::UpToSurface => {
                let (plane, _, _) = self.target_plane(target, st)?;
                to_plane(plane.origin, plane.z)
            }
            EndCondition::OffsetFromSurface => {
                let (plane, reverse, translate) = self.target_plane(target, st)?;
                let d = num(depth, "offset")?;
                if d < 0.0 {
                    return Err("the offset must not be negative".into());
                }
                // Short of the surface (towards the sketch) unless reversed.
                let sign = if reverse { 1.0 } else { -1.0 };
                let origin = if translate {
                    plane.origin + u * (sign * d)
                } else {
                    // Along the plane's own normal, turned to face away from the sketch.
                    let away = plane.z * u.dot(plane.z).signum();
                    plane.origin + away * (sign * d)
                };
                to_plane(origin, plane.z)
            }
            EndCondition::UpToNext => self.up_to_next(start, u, profiles, st, &to_plane),
        }
    }

    /// The nearest face of the part that every point of the profile runs into.
    fn up_to_next(
        &self,
        start: &Frame,
        u: Vec3,
        profiles: &[Profile],
        st: &BuildState,
        to_plane: &dyn Fn(Vec3, Vec3) -> Result<Reach, String>,
    ) -> Result<Reach, String> {
        let rays = inner_points(start, profiles);
        if rays.is_empty() || st.body.is_empty() {
            return Err("no face covers the whole profile in this direction".into());
        }
        // For each ray, the nearest hit on each face.
        let mut common: Option<std::collections::BTreeMap<anvil_kernel::FaceId, f64>> = None;
        for o in &rays {
            let mut hits: std::collections::BTreeMap<anvil_kernel::FaceId, f64> = Default::default();
            for p in &st.body.polygons {
                let den = p.normal.dot(u);
                if den.abs() < 1e-12 {
                    continue;
                }
                let t = (p.w - p.normal.dot(*o)) / den;
                if t <= 1e-6 {
                    continue;
                }
                let hit = *o + u * t;
                let k = p.verts.len();
                let inside = (0..k).all(|i| {
                    let (a, b) = (p.verts[i], p.verts[(i + 1) % k]);
                    (b - a).cross(hit - a).dot(p.normal) >= -1e-9
                });
                if inside {
                    let e = hits.entry(p.tag.id).or_insert(t);
                    *e = e.min(t);
                }
            }
            common = Some(match common {
                None => hits,
                Some(c) => {
                    c.into_iter().filter_map(|(f, t)| hits.get(&f).map(|&t2| (f, t.max(t2)))).collect()
                }
            });
        }
        let best = common
            .unwrap_or_default()
            .into_iter()
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .ok_or("no face covers the whole profile in this direction")?;
        let frame = st
            .body
            .planar_face_frame(best.0)
            .ok_or("the next face is curved; Up To Next works up to flat faces")?;
        to_plane(frame.origin, frame.z)
    }

    /// Plans an extrusion: its regions and pieces.
    pub(crate) fn plan_extrude(&self, ef: &ExtrudeFeature, st: &BuildState) -> Result<Plan, String> {
        let num = |d: &Dim, what: &str| d.eval(&st.variables).map_err(|e| format!("{what}: {e}"));
        let sf = self.sketch(ef.sketch).ok_or("the sketch this feature uses is missing")?;
        let sketch_frame =
            *st.frames.get(&ef.sketch).ok_or("the sketch this feature uses failed or is suppressed")?;
        let boss = ef.op == BodyOp::Boss;

        let mut capped = None;
        let profiles = match &ef.thin {
            Some(w) => {
                let t1 = num(&w.thickness, "thickness")?;
                let t2 = num(&w.thickness2, "second thickness")?;
                let fillet = w.fillet.as_ref().map(|f| num(f, "fillet radius")).transpose()?;
                let walls = crate::thin::wall_profiles(&sf.sketch, w, t1, t2, fillet)?;
                if let Some(c) = &w.cap {
                    let c = num(c, "cap thickness")?;
                    if c <= 0.0 {
                        return Err("the cap thickness must be greater than zero".into());
                    }
                    let hollows = crate::thin::hollows(&walls);
                    if hollows.is_empty() {
                        return Err("cap ends need a closed contour".into());
                    }
                    capped = Some((crate::thin::outlines(&walls), hollows, c));
                }
                walls
            }
            None => match sf.sketch.profiles() {
                Ok(p) => p,
                Err(anvil_sketch::ProfileError::OpenContour { .. }) => {
                    return Err("open profile: use a thin feature".into());
                }
                Err(e) => return Err(e.to_string()),
            },
        };

        // The start plane.
        let start = match &ef.start_offset {
            Some(d) => {
                let d = num(d, "start offset")?;
                sketch_frame.offset(if ef.start_flip { -d } else { d })
            }
            None => sketch_frame,
        };
        // Direction 1: along the sketch normal for a boss, against it for a cut.
        let s1 = match (boss, ef.reverse) {
            (true, false) | (false, true) => 1.0,
            _ => -1.0,
        };
        let u1 = start.z * s1;
        let r1 = self.reach(ef.end, &ef.depth, &ef.target, &start, u1, boss, &profiles, st)?;
        let r2 = if ef.end.two_sided() {
            Some(self.reach(ef.end, &ef.depth, &ef.target, &start, -u1, boss, &profiles, st)?)
        } else if let Some(d2) = &ef.direction2 {
            if d2.end.two_sided() {
                return Err("Direction 2 cannot be two-sided".into());
            }
            Some(self.reach(d2.end, &d2.depth, &d2.target, &start, -u1, boss, &profiles, st)?)
        } else {
            None
        };
        let taper = |draft: &Option<Draft>, s: f64| -> Result<f64, String> {
            match draft {
                None => Ok(0.0),
                Some(d) => {
                    let a = num(&d.angle, "draft angle")?;
                    if !(0.0..90.0).contains(&a) {
                        return Err("the draft angle must be between 0 and 90 degrees".into());
                    }
                    let t = a.to_radians().tan();
                    // Inward closes the section in going away from the start.
                    Ok(if d.outward { s * t } else { -s * t })
                }
            }
        };
        let draft2 = if ef.end.two_sided() {
            ef.draft.clone()
        } else {
            ef.direction2.as_ref().and_then(|d| d.draft.clone())
        };
        let t1 = taper(&ef.draft, s1)?;
        let t2 = taper(&draft2, -s1)?;

        let piece = |r: &Reach, s: f64, t: f64| -> Result<Piece, String> {
            match *r {
                Reach::Distance(d) => Ok(Piece { z0: 0.0, z1: s * d, taper: t, trim: None }),
                Reach::Plane(trim, far) => {
                    if t != 0.0 {
                        return Err("draft needs an end parallel to the sketch".into());
                    }
                    Ok(Piece { z0: 0.0, z1: s * far, taper: 0.0, trim: Some(trim) })
                }
            }
        };
        let mut pieces = vec![piece(&r1, s1, t1)?];
        if let Some(r2) = &r2 {
            pieces.push(piece(r2, -s1, t2)?);
        }
        // Two plain runs make one prism, named as a single extrusion.
        if let [a, b] = pieces.as_slice() {
            if a.trim.is_none() && b.trim.is_none() && a.taper == 0.0 && b.taper == 0.0 {
                let (lo, hi) = (a.z1.min(b.z1), a.z1.max(b.z1));
                // Mid Plane and Through All - Both run up the sketch normal, as they
                // always have; otherwise the end cap is at Direction 1's end.
                pieces = vec![if a.z1 >= b.z1 || ef.end.two_sided() {
                    Piece { z0: lo, z1: hi, taper: 0.0, trim: None }
                } else {
                    Piece { z0: hi, z1: lo, taper: 0.0, trim: None }
                }];
            }
        }
        if capped.is_some() && (pieces.len() > 1 || pieces[0].trim.is_some() || pieces[0].taper != 0.0) {
            return Err("cap ends need a plain extrusion (no draft, one run, flat ends)".into());
        }
        if ef.flip_side && boss {
            return Err("flip side to cut is for cuts".into());
        }
        if ef.flip_side
            && (pieces.len() > 1
                || pieces.iter().any(|p| p.trim.is_some() || p.taper != 0.0)
                || ef.thin.is_some())
        {
            return Err("flip side to cut needs a solid profile with flat ends".into());
        }
        let b = st.body.bounds();
        let mut reach = if b.is_empty() { 0.0 } else { b.diagonal() + (b.center() - start.origin).length() };
        for p in world_points(&start, &profiles) {
            reach = reach.max(2.0 * (p - start.origin).length());
        }
        Ok(Plan { frame: start, profiles, pieces, capped, flip: ef.flip_side, reach: reach + 10.0 })
    }
}

/// A square slab in `frame`'s plane, centred on its origin, from `z0` to `z1`.
fn slab_profile(half: f64) -> Profile {
    Profile::new(Loop2::rectangle(v2(-half, -half), v2(half, half), 0), vec![])
}

/// A half-space as a big box: the side of the trim plane that is kept.
fn trim_box(t: &Trim, near: Vec3, reach: f64) -> (Frame, Profile, f64, f64) {
    let origin = near + t.normal * (t.origin - near).dot(t.normal);
    let hint = if t.normal.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let frame = Frame::from_normal(origin, t.normal, hint);
    (frame, slab_profile(2.0 * reach), -4.0 * reach, 0.0)
}

impl Plan {
    /// The extruded tool on the faceted kernel.
    pub(crate) fn faceted(&self, id: u32) -> Result<Solid, String> {
        let e = |r: anvil_kernel::build::BuildError| r.to_string();
        let mut tool: Option<Solid> = None;
        for p in &self.pieces {
            let mut s = match &self.capped {
                Some((outer, hollow, cap)) => {
                    let (lo, hi) = (p.z0.min(p.z1), p.z0.max(p.z1));
                    if hi - lo <= 2.0 * cap {
                        return Err("the caps are thicker than the extrusion is long".into());
                    }
                    let full = extrude_drafted(outer, &self.frame, p.z0, p.z1, 0.0, id).map_err(e)?;
                    let inside =
                        extrude_drafted(hollow, &self.frame, lo + cap, hi - cap, 0.0, id).map_err(e)?;
                    full.subtract(&inside)
                }
                None => extrude_drafted(&self.profiles, &self.frame, p.z0, p.z1, p.taper, id).map_err(e)?,
            };
            if let Some(t) = &p.trim {
                let (f, prof, z0, z1) = trim_box(t, self.frame.origin, self.reach);
                s = s.intersect(&extrude_drafted(&[prof], &f, z0, z1, 0.0, id).map_err(e)?);
            }
            tool = Some(match tool {
                None => s,
                Some(t) => t.union(&s),
            });
        }
        let mut tool = tool.ok_or("the extrusion has no length")?;
        if self.flip {
            let p = self.pieces[0];
            let slab =
                extrude_drafted(&[slab_profile(self.reach)], &self.frame, p.z0, p.z1, 0.0, id).map_err(e)?;
            tool = slab.subtract(&tool);
        }
        Ok(tool)
    }

    /// The extruded tool bodies on the exact kernel.
    pub(crate) fn exact(&self, id: u32) -> Result<Vec<brep::Solid>, String> {
        let union_all = |a: Vec<brep::Solid>, b: Vec<brep::Solid>| -> Result<Vec<brep::Solid>, String> {
            let mut out = a;
            for t in b {
                out = exact::combine(&out, &t, BodyOp::Boss)?;
            }
            Ok(out)
        };
        let mut tools: Vec<brep::Solid> = Vec::new();
        for p in &self.pieces {
            let mut s = match &self.capped {
                Some((outer, hollow, cap)) => {
                    let (lo, hi) = (p.z0.min(p.z1), p.z0.max(p.z1));
                    if hi - lo <= 2.0 * cap {
                        return Err("the caps are thicker than the extrusion is long".into());
                    }
                    let full = exact::extrude(outer, &self.frame, p.z0, p.z1, id)?;
                    let inside = exact::extrude(hollow, &self.frame, lo + cap, hi - cap, id)?;
                    let mut out = full;
                    for h in &inside {
                        out = exact::combine(&out, h, BodyOp::Cut)?;
                    }
                    out
                }
                None => exact::extrude_drafted(&self.profiles, &self.frame, p.z0, p.z1, p.taper, id)?,
            };
            if let Some(t) = &p.trim {
                let (f, prof, z0, z1) = trim_box(t, self.frame.origin, self.reach);
                let half = exact::extrude(&[prof], &f, z0, z1, id)?;
                let mut kept = Vec::new();
                for body in &s {
                    for h in &half {
                        kept.extend(boolean(body, h, BoolOp::Intersect).map_err(|e| e.to_string())?);
                    }
                }
                s = kept;
            }
            tools = if tools.is_empty() { s } else { union_all(tools, s)? };
        }
        if self.flip {
            let p = self.pieces[0];
            let slab = exact::extrude(&[slab_profile(self.reach)], &self.frame, p.z0, p.z1, id)?;
            let mut out = slab;
            for t in &tools {
                out = exact::combine(&out, t, BodyOp::Cut)?;
            }
            tools = out;
        }
        if tools.is_empty() {
            return Err("the extrusion has no volume".into());
        }
        Ok(tools)
    }
}

#[cfg(test)]
mod tests {
    //! The testable statements of `docs/re/extrude-revolve.md`, numbered as there.
    use super::*;
    use anvil_kernel::FaceId;
    use anvil_sketch::Sketch;

    /// A sketch with one rectangle, on `plane`.
    fn rect_sketch(doc: &mut Document, plane: PlaneRef, a: (f64, f64), b: (f64, f64)) -> u32 {
        let mut sf = SketchFeature::new(plane);
        sf.sketch.add_rectangle(v2(a.0, a.1), v2(b.0, b.1));
        doc.add_feature(FeatureKind::Sketch(sf))
    }

    fn circle_sketch(doc: &mut Document, plane: PlaneRef, r: f64) -> u32 {
        let mut sf = SketchFeature::new(plane);
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        sf.sketch.add_circle(c, r);
        doc.add_feature(FeatureKind::Sketch(sf))
    }

    fn extrude(doc: &mut Document, sketch: u32, f: ExtrudeFeature) -> u32 {
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature { sketch, ..f }))
    }

    fn blind(depth: f64) -> ExtrudeFeature {
        ExtrudeFeature { depth: Dim::Value(depth), ..Default::default() }
    }

    /// Rebuilds, checks there were no errors, and returns the volume and z range.
    fn built(doc: &mut Document) -> (f64, f64, f64, BuildState) {
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let b = st.body.bounds();
        (st.body.mass_properties().volume, b.min.z, b.max.z, st)
    }

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1.0)
    }

    /// The 20 x 10 rectangle on the Front plane (normal +z).
    fn rect_doc() -> (Document, u32) {
        let mut doc = Document::new();
        let s = rect_sketch(&mut doc, PlaneRef::Front, (0.0, 0.0), (20.0, 10.0));
        (doc, s)
    }

    /// The 40 x 40 x 20 block, x and y from -10 to 30, z from 0 to 20.
    fn block_doc() -> (Document, u32) {
        let mut doc = Document::new();
        let s = rect_sketch(&mut doc, PlaneRef::Front, (-10.0, -10.0), (30.0, 30.0));
        let b = extrude(&mut doc, s, blind(20.0));
        (doc, b)
    }

    fn top_of(block: u32) -> PlaneRef {
        PlaneRef::Face(FaceId { feature: block, index: FaceId::END_CAP })
    }

    #[test]
    fn s01_s02_s04_s05_blind_reverse_mid_plane_and_two_directions() {
        let (mut doc, s) = rect_doc();
        let e = extrude(&mut doc, s, blind(10.0));
        let (v, lo, hi, st) = built(&mut doc);
        assert!(close(v, 2000.0, 1e-9) && lo.abs() < 1e-9 && (hi - 10.0).abs() < 1e-9);
        assert!(st.exact.is_some());
        let set = |doc: &mut Document, f: &dyn Fn(&mut ExtrudeFeature)| {
            if let Some(FeatureKind::Extrude(x)) = doc.feature_mut(e).map(|f| &mut f.kind) {
                f(x);
            }
        };
        set(&mut doc, &|x| x.reverse = true);
        let (v, lo, hi, _) = built(&mut doc);
        assert!(close(v, 2000.0, 1e-9) && (lo + 10.0).abs() < 1e-9 && hi.abs() < 1e-9);
        set(&mut doc, &|x| {
            x.reverse = false;
            x.end = EndCondition::MidPlane;
        });
        let (v, lo, hi, _) = built(&mut doc);
        assert!(close(v, 2000.0, 1e-9) && (lo + 5.0).abs() < 1e-9 && (hi - 5.0).abs() < 1e-9);
        set(&mut doc, &|x| {
            x.end = EndCondition::Blind;
            x.direction2 =
                Some(EndSpec { end: EndCondition::Blind, depth: Dim::Value(5.0), target: None, draft: None });
        });
        let (v, lo, hi, _) = built(&mut doc);
        assert!(
            close(v, 3000.0, 1e-9) && (lo + 5.0).abs() < 1e-9 && (hi - 10.0).abs() < 1e-9,
            "{v} {lo} {hi}"
        );
    }

    #[test]
    fn s03_s06_s07_cuts_from_the_top_face() {
        let (mut doc, b) = block_doc();
        let s = rect_sketch(&mut doc, top_of(b), (0.0, 0.0), (20.0, 10.0));
        let c = extrude(&mut doc, s, ExtrudeFeature { op: BodyOp::Cut, ..blind(5.0) });
        let (v, ..) = built(&mut doc);
        assert!(close(v, 31000.0, 1e-9), "{v}");
        if let Some(FeatureKind::Extrude(x)) = doc.feature_mut(c).map(|f| &mut f.kind) {
            x.end = EndCondition::ThroughAll;
        }
        let (v, ..) = built(&mut doc);
        assert!(close(v, 28000.0, 1e-9), "{v}");
        if let Some(FeatureKind::Extrude(x)) = doc.feature_mut(c).map(|f| &mut f.kind) {
            x.flip_side = true;
        }
        let (v, _, _, st) = built(&mut doc);
        assert!(close(v, 4000.0, 1e-9), "{v}");
        assert!(st.exact.is_some());
    }

    #[test]
    fn s06_through_all_both_from_the_middle() {
        let (mut doc, _) = block_doc();
        let mid = doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base: PlaneRef::Front,
            offset: Dim::Value(10.0),
            flip: false,
        }));
        let s = rect_sketch(&mut doc, PlaneRef::Feature(mid), (0.0, 0.0), (20.0, 10.0));
        extrude(
            &mut doc,
            s,
            ExtrudeFeature { op: BodyOp::Cut, end: EndCondition::ThroughAllBoth, ..blind(1.0) },
        );
        let (v, ..) = built(&mut doc);
        assert!(close(v, 28000.0, 1e-9), "{v}");
    }

    #[test]
    fn s08_through_all_boss_ends_with_the_part() {
        let (mut doc, _) = block_doc();
        // A plane at z = -5 facing +z.
        let below = doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base: PlaneRef::Front,
            offset: Dim::Value(-5.0),
            flip: false,
        }));
        let s = rect_sketch(&mut doc, PlaneRef::Feature(below), (0.0, 0.0), (20.0, 10.0));
        let e = extrude(&mut doc, s, ExtrudeFeature { end: EndCondition::ThroughAll, ..blind(1.0) });
        let (v, lo, hi, _) = built(&mut doc);
        assert!(
            close(v, 33000.0, 1e-9) && (lo + 5.0).abs() < 1e-9 && (hi - 20.0).abs() < 1e-9,
            "{v} {lo} {hi}"
        );
        // Run away from the block there is nothing to go through.
        if let Some(FeatureKind::Extrude(x)) = doc.feature_mut(e).map(|f| &mut f.kind) {
            x.reverse = true;
        }
        assert!(doc.rebuild().errors.contains_key(&e));
    }

    fn drafted(angle: f64, outward: bool) -> Option<Draft> {
        Some(Draft { angle: Dim::Value(angle), outward })
    }

    #[test]
    fn s09_s10_s13_drafted_rectangle() {
        let (mut doc, s) = rect_doc();
        let e = extrude(&mut doc, s, ExtrudeFeature { draft: drafted(5.0, false), ..blind(10.0) });
        let (v, _, _, st) = built(&mut doc);
        assert!(close(v, 1747.740, 1e-6), "{v}");
        assert!(st.exact.is_some());
        let set = |doc: &mut Document, depth: f64, outward: bool| {
            if let Some(FeatureKind::Extrude(x)) = doc.feature_mut(e).map(|f| &mut f.kind) {
                x.depth = Dim::Value(depth);
                x.draft = drafted(5.0, outward);
            }
        };
        set(&mut doc, 10.0, true);
        let (v, ..) = built(&mut doc);
        assert!(close(v, 2272.672, 1e-6), "{v}");
        set(&mut doc, 57.0, false);
        built(&mut doc);
        set(&mut doc, 58.0, false);
        assert!(doc.rebuild().errors.contains_key(&e));
    }

    #[test]
    fn s11_drafted_circle_is_a_cone() {
        let mut doc = Document::new();
        let s = circle_sketch(&mut doc, PlaneRef::Front, 10.0);
        let e = extrude(&mut doc, s, ExtrudeFeature { draft: drafted(5.0, false), ..blind(10.0) });
        let (v, _, _, st) = built(&mut doc);
        assert!(close(v, 2874.754, 2e-3), "{v}");
        assert!(st.exact.as_ref().is_some_and(|b| b[0]
            .faces
            .iter()
            .any(|f| matches!(f.surface, anvil_brep::Surface::Cone { .. }))));
        if let Some(FeatureKind::Extrude(x)) = doc.feature_mut(e).map(|f| &mut f.kind) {
            x.draft = drafted(5.0, true);
        }
        let (v, ..) = built(&mut doc);
        assert!(close(v, 3424.462, 2e-3), "{v}");
    }

    #[test]
    fn s12_drafted_mid_plane() {
        let (mut doc, s) = rect_doc();
        let e = extrude(
            &mut doc,
            s,
            ExtrudeFeature { end: EndCondition::MidPlane, draft: drafted(5.0, false), ..blind(10.0) },
        );
        let (v, lo, hi, _) = built(&mut doc);
        assert!(close(v, 1871.318, 1e-6) && (lo + 5.0).abs() < 1e-9 && (hi - 5.0).abs() < 1e-9, "{v}");
        if let Some(FeatureKind::Extrude(x)) = doc.feature_mut(e).map(|f| &mut f.kind) {
            x.draft = drafted(5.0, true);
        }
        let (v, ..) = built(&mut doc);
        assert!(close(v, 2133.784, 1e-6), "{v}");
    }

    #[test]
    fn s14_start_offset() {
        let (mut doc, s) = rect_doc();
        extrude(&mut doc, s, ExtrudeFeature { start_offset: Some(Dim::Value(5.0)), ..blind(10.0) });
        let (v, lo, hi, _) = built(&mut doc);
        assert!(close(v, 2000.0, 1e-9) && (lo - 5.0).abs() < 1e-9 && (hi - 15.0).abs() < 1e-9);
    }

    #[test]
    fn s15_up_to_vertex() {
        let (mut doc, s) = rect_doc();
        let at = |z: f64| Some(EndTarget::Point(PointRef::Model(Vec3 { x: 35.0, y: 35.0, z })));
        let e = extrude(
            &mut doc,
            s,
            ExtrudeFeature { end: EndCondition::UpToVertex, target: at(7.0), ..blind(1.0) },
        );
        let (v, _, hi, _) = built(&mut doc);
        assert!(close(v, 1400.0, 1e-9) && (hi - 7.0).abs() < 1e-9);
        if let Some(FeatureKind::Extrude(x)) = doc.feature_mut(e).map(|f| &mut f.kind) {
            x.target = at(-3.0);
        }
        assert!(doc.rebuild().errors.contains_key(&e));
    }

    /// The plane z = 10 + 0.5 x.
    fn leaning() -> PlaneRef {
        let n = Vec3 { x: -0.5, y: 0.0, z: 1.0 }.normalized();
        PlaneRef::Fixed(Frame::from_normal(Vec3 { x: 0.0, y: 0.0, z: 10.0 }, n, Vec3::X))
    }

    #[test]
    fn s16_s17_up_to_and_offset_from_a_leaning_plane() {
        let (mut doc, s) = rect_doc();
        let target = |reverse_offset, translate| {
            Some(EndTarget::Plane { plane: leaning(), reverse_offset, translate })
        };
        let e = extrude(
            &mut doc,
            s,
            ExtrudeFeature { end: EndCondition::UpToSurface, target: target(false, false), ..blind(1.0) },
        );
        let (v, _, hi, st) = built(&mut doc);
        assert!(close(v, 3000.0, 1e-9) && (hi - 20.0).abs() < 1e-9, "{v} {hi}");
        assert!(st.exact.is_some());
        let set = |doc: &mut Document, translate| {
            if let Some(FeatureKind::Extrude(x)) = doc.feature_mut(e).map(|f| &mut f.kind) {
                x.end = EndCondition::OffsetFromSurface;
                x.depth = Dim::Value(2.0);
                x.target = target(false, translate);
            }
        };
        set(&mut doc, false);
        let (v, ..) = built(&mut doc);
        assert!(close(v, 2552.786, 1e-6), "{v}");
        set(&mut doc, true);
        let (v, ..) = built(&mut doc);
        assert!(close(v, 2600.0, 1e-9), "{v}");
    }

    #[test]
    fn s18_offset_from_a_parallel_plane() {
        let (mut doc, s) = rect_doc();
        let p30 = PlaneRef::Fixed(Frame::WORLD.offset(30.0));
        let e = extrude(
            &mut doc,
            s,
            ExtrudeFeature {
                end: EndCondition::OffsetFromSurface,
                target: Some(EndTarget::Plane { plane: p30, reverse_offset: false, translate: false }),
                ..blind(5.0)
            },
        );
        let (v, _, hi, _) = built(&mut doc);
        assert!(close(v, 5000.0, 1e-9) && (hi - 25.0).abs() < 1e-9);
        if let Some(FeatureKind::Extrude(x)) = doc.feature_mut(e).map(|f| &mut f.kind) {
            x.target = Some(EndTarget::Plane { plane: p30, reverse_offset: true, translate: false });
        }
        let (v, _, hi, _) = built(&mut doc);
        assert!(close(v, 7000.0, 1e-9) && (hi - 35.0).abs() < 1e-9);
    }

    #[test]
    fn s19_up_to_next() {
        for (x0, covers) in [(-10.0, true), (10.0, false)] {
            let mut doc = Document::new();
            let up = doc.add_feature(FeatureKind::Plane(PlaneFeature {
                base: PlaneRef::Front,
                offset: Dim::Value(20.0),
                flip: false,
            }));
            let b = rect_sketch(&mut doc, PlaneRef::Feature(up), (x0, -10.0), (30.0, 30.0));
            extrude(&mut doc, b, blind(5.0));
            let s = rect_sketch(&mut doc, PlaneRef::Front, (0.0, 0.0), (20.0, 10.0));
            let e = extrude(&mut doc, s, ExtrudeFeature { end: EndCondition::UpToNext, ..blind(1.0) });
            let st = doc.rebuild();
            if covers {
                assert!(st.errors.is_empty(), "{:?}", st.errors);
                assert!(close(st.body.mass_properties().volume, 8000.0 + 4000.0, 1e-9));
            } else {
                assert!(st.errors.contains_key(&e));
            }
        }
    }

    fn thin(kind: ThinKind, t1: f64, t2: f64, reverse: bool) -> Option<ThinWall> {
        Some(ThinWall {
            kind,
            thickness: Dim::Value(t1),
            thickness2: Dim::Value(t2),
            reverse,
            fillet: None,
            cap: None,
        })
    }

    #[test]
    fn s21_s22_thin_closed_rectangle() {
        for (w, vol) in [
            (thin(ThinKind::OneDirection, 1.0, 0.0, false), 640.0),
            (thin(ThinKind::OneDirection, 1.0, 0.0, true), 560.0),
            (thin(ThinKind::MidPlane, 1.0, 0.0, false), 600.0),
            (thin(ThinKind::TwoDirection, 1.0, 2.0, false), 1680.0),
        ] {
            let (mut doc, s) = rect_doc();
            extrude(&mut doc, s, ExtrudeFeature { thin: w, ..blind(10.0) });
            let (v, _, _, st) = built(&mut doc);
            assert!(close(v, vol, 1e-9), "{v} vs {vol}");
            assert!(st.exact.is_some());
            assert!(doc.features[1].name.starts_with("Extrude-Thin"));
        }
        let (mut doc, s) = rect_doc();
        let mut w = thin(ThinKind::OneDirection, 1.0, 0.0, true);
        if let Some(w) = &mut w {
            w.cap = Some(Dim::Value(1.0));
        }
        extrude(&mut doc, s, ExtrudeFeature { thin: w, ..blind(10.0) });
        let (v, _, _, st) = built(&mut doc);
        assert!(close(v, 848.0, 1e-9), "{v}");
        assert_eq!(st.body.open_edges(), 0);
    }

    fn l_doc() -> (Document, u32) {
        let mut doc = Document::new();
        let mut sk = Sketch::new();
        let a = sk.add_point(v2(0.0, 10.0));
        let b = sk.add_point(v2(0.0, 0.0));
        let c = sk.add_point(v2(20.0, 0.0));
        sk.add_line(a, b);
        sk.add_line(b, c);
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sf.sketch = sk;
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        (doc, s)
    }

    #[test]
    fn s23_s24_s25_thin_open_l() {
        for (w, fillet, vol) in [
            (thin(ThinKind::MidPlane, 2.0, 0.0, false), None, 600.0),
            (thin(ThinKind::OneDirection, 2.0, 0.0, false), None, 560.0),
            (thin(ThinKind::OneDirection, 2.0, 0.0, true), None, 640.0),
            (thin(ThinKind::OneDirection, 2.0, 0.0, false), Some(1.0), 542.832),
            (thin(ThinKind::OneDirection, 2.0, 0.0, true), Some(1.0), 622.832),
        ] {
            let (mut doc, s) = l_doc();
            let mut w = w;
            if let Some(w) = &mut w {
                w.fillet = fillet.map(Dim::Value);
            }
            extrude(&mut doc, s, ExtrudeFeature { thin: w, ..blind(10.0) });
            let (v, ..) = built(&mut doc);
            assert!(close(v, vol, 1e-3), "{v} vs {vol}");
        }
    }

    #[test]
    fn s33_open_profile_needs_thin() {
        let (mut doc, s) = l_doc();
        let e = extrude(&mut doc, s, blind(10.0));
        let st = doc.rebuild();
        assert!(st.errors.get(&e).is_some_and(|m| m.contains("thin")));
    }

    #[test]
    fn old_files_keep_their_meaning() {
        // A part saved before these options existed has none of the new fields.
        let json = r#"{"sketch":1,"end":"Blind","depth":{"Value":10.0},"reverse":false,"op":"Boss"}"#;
        let e: ExtrudeFeature = serde_json::from_str(json).unwrap();
        assert_eq!(e, ExtrudeFeature { sketch: 1, ..blind(10.0) });
    }
}
