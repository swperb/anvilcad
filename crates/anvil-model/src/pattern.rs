//! Patterns and mirrors: copies of seed features' tool bodies, moved or reflected, and
//! joined to or cut from the part as their seed was. The behaviour is specified in
//! `docs/re/mirror-and-patterns.md`.
//!
//! Every pattern and mirror keeps the union of its copies as its own tool, so it can be
//! a seed itself (a mirror of a pattern, say).

use crate::document::*;
use crate::exact;
use crate::rebuild::BuildState;
use anvil_brep as brep;
use anvil_kernel::{Frame, Solid, Vec3};

/// The most instances one pattern may make: a guard against a typing slip.
const MAX_INSTANCES: usize = 10_000;

/// Where one copy goes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Place {
    /// A rigid motion.
    Move(Frame),
    /// A reflection in the plane through `.0` with normal `.1`.
    Reflect(Vec3, Vec3),
    /// A reflection in one plane, then in another.
    Twice((Vec3, Vec3), (Vec3, Vec3)),
}

impl Place {
    fn faceted(&self, s: &Solid) -> Solid {
        match *self {
            Place::Move(f) => s.transformed(&f),
            Place::Reflect(o, n) => s.mirrored(o, n),
            Place::Twice((o1, n1), (o2, n2)) => s.mirrored(o1, n1).mirrored(o2, n2),
        }
    }

    fn exact(&self, s: &brep::Solid) -> Result<brep::Solid, String> {
        let d = |v: Vec3| glam::DVec3::new(v.x, v.y, v.z);
        let e = |k: brep::KernelError| k.to_string();
        match *self {
            Place::Move(f) => Ok(s.transformed(&exact::frame(&f))),
            Place::Reflect(o, n) => brep::mirror::mirrored(s, d(o), d(n)).map_err(e),
            Place::Twice((o1, n1), (o2, n2)) => {
                let once = brep::mirror::mirrored(s, d(o1), d(n1)).map_err(e)?;
                brep::mirror::mirrored(&once, d(o2), d(n2)).map_err(e)
            }
        }
    }
}

/// `p` turned by `angle` radians about the line through `origin` along unit `axis`.
pub(crate) fn rotation_frame(origin: Vec3, axis: Vec3, angle: f64) -> Frame {
    let rot = |v: Vec3| {
        let (s, c) = angle.sin_cos();
        v * c + axis.cross(v) * s + axis * axis.dot(v) * (1.0 - c)
    };
    Frame { origin: origin - rot(origin), x: rot(Vec3::X), y: rot(Vec3::Y), z: rot(Vec3::Z) }
}

fn translation(v: Vec3) -> Frame {
    Frame { origin: v, ..Frame::WORLD }
}

impl BuildState {
    /// Places a copy of every seed's tool at each place, applying each as its seed was.
    pub(crate) fn place_copies(&mut self, id: u32, seeds: &[u32], places: &[Place]) -> Result<(), String> {
        if seeds.is_empty() {
            return Err("choose at least one feature to copy".into());
        }
        if places.len() > MAX_INSTANCES {
            return Err(format!("more than {MAX_INSTANCES} instances"));
        }
        let faceted: Vec<(Solid, BodyOp)> = seeds
            .iter()
            .map(|s| {
                self.tools.get(s).cloned().ok_or_else(|| "a seed feature is missing or failed".to_string())
            })
            .collect::<Result<_, _>>()?;
        let exact_seeds: Result<Vec<(Vec<brep::Solid>, BodyOp)>, String> = seeds
            .iter()
            .map(|s| {
                self.exact_tools
                    .get(s)
                    .cloned()
                    .ok_or_else(|| "a seed feature was not built exactly".to_string())
            })
            .collect();
        let mut exact_copies: Vec<brep::Solid> = Vec::new();
        let done = self.exact_step(id, |bodies| {
            let mut out = bodies.to_vec();
            for (tools, op) in &exact_seeds? {
                for p in places {
                    for t in tools {
                        let c = p.exact(t)?;
                        out = exact::combine(&out, &c, *op)?;
                        exact_copies.push(c);
                    }
                }
            }
            Ok(out)
        });
        let mut polys = Vec::new();
        for (tool, op) in &faceted {
            for p in places {
                let c = p.faceted(tool);
                if !done {
                    crate::rebuild::apply(&mut self.body, &c, *op)?;
                }
                polys.extend(c.polygons);
            }
        }
        // Keep the copies as this feature's tool, so it can be a seed in turn.
        let op = faceted[0].1;
        if faceted.iter().all(|(_, o)| *o == op) {
            self.tools.insert(id, (Solid::new(polys), op));
            if done {
                self.exact_tools.insert(id, (exact_copies, op));
            }
        }
        Ok(())
    }
}

impl Document {
    pub(crate) fn rebuild_linear_pattern(
        &self,
        id: u32,
        pf: &LinearPatternFeature,
        st: &mut BuildState,
    ) -> Result<(), String> {
        let num = |d: &Dim, what: &str| d.eval(&st.variables).map_err(|e| format!("{what}: {e}"));
        let count = |d: &Dim| -> Result<usize, String> {
            let n = num(d, "instances")?.round();
            if !(1.0..=MAX_INSTANCES as f64).contains(&n) {
                return Err(format!("the number of instances must be from 1 to {MAX_INSTANCES}"));
            }
            Ok(n as usize)
        };
        let spacing = |d: &Dim| -> Result<f64, String> {
            let s = num(d, "spacing")?;
            if s <= 0.0 {
                return Err("the spacing must be greater than zero".into());
            }
            Ok(s)
        };
        let d1 = self.resolve_axis(&pf.direction, st)?.1 * if pf.reverse { -1.0 } else { 1.0 };
        let (n1, s1) = (count(&pf.count)?, spacing(&pf.spacing)?);
        let (d2, n2, s2) = match &pf.direction2 {
            Some(p) => {
                let d2 = self.resolve_axis(&p.direction, st)?.1 * if p.reverse { -1.0 } else { 1.0 };
                if d2.cross(d1).length() < 1e-9 {
                    return Err("Direction 2 runs the same way as Direction 1".into());
                }
                (d2, count(&p.count)?, spacing(&p.spacing)?)
            }
            None => (Vec3::ZERO, 1, 0.0),
        };
        if n1 * n2 > MAX_INSTANCES {
            return Err(format!("more than {MAX_INSTANCES} instances"));
        }
        let mut places = Vec::new();
        for i in 1..=n1 {
            for j in 1..=n2 {
                if (i, j) == (1, 1)
                    || (pf.seed_only && i > 1 && j > 1)
                    || pf.skip.contains(&[i as u32, j as u32])
                {
                    continue;
                }
                places
                    .push(Place::Move(translation(d1 * (s1 * (i - 1) as f64) + d2 * (s2 * (j - 1) as f64))));
            }
        }
        st.place_copies(id, &pf.features, &places)
    }

    pub(crate) fn rebuild_circular_pattern(
        &self,
        id: u32,
        pf: &CircularPatternFeature,
        st: &mut BuildState,
    ) -> Result<(), String> {
        let num = |d: &Dim, what: &str| d.eval(&st.variables).map_err(|e| format!("{what}: {e}"));
        // The angle between neighbours, for `n` instances and an angle `a` (degrees).
        let step = |a: f64, n: usize, equal: bool| -> f64 {
            if !equal {
                a
            } else if (a.abs() - 360.0).abs() < 1e-9 {
                a / n as f64
            } else {
                a / (n.max(2) - 1) as f64
            }
        };
        let count = |d: &Dim| -> Result<usize, String> {
            let n = num(d, "instances")?.round();
            if !(1.0..=MAX_INSTANCES as f64).contains(&n) {
                return Err(format!("the number of instances must be from 1 to {MAX_INSTANCES}"));
            }
            Ok(n as usize)
        };
        let sense = if pf.reverse { -1.0 } else { 1.0 };
        let (a1, n1) = (num(&pf.total_angle, "angle")?, count(&pf.count)?);
        let s1 = sense * step(a1, n1, pf.equal_spacing);
        let mut angles: Vec<(u32, usize, f64)> = (2..=n1).map(|k| (1, k, s1 * (k - 1) as f64)).collect();
        if let Some(d2) = &pf.direction2 {
            let (a2, n2, eq) = if d2.symmetric {
                (a1, n1, pf.equal_spacing)
            } else {
                (num(&d2.angle, "Direction 2 angle")?, count(&d2.count)?, d2.equal_spacing)
            };
            let s2 = -sense * step(a2, n2, eq);
            angles.extend((2..=n2).map(|k| (2, k, s2 * (k - 1) as f64)));
        }
        let (origin, axis) = self.resolve_axis(&pf.axis, st)?;
        let places: Vec<Place> = angles
            .into_iter()
            .filter(|(dir, k, _)| !pf.skip.contains(&[*dir, *k as u32]))
            .map(|(_, _, a)| Place::Move(rotation_frame(origin, axis, a.to_radians())))
            .collect();
        st.place_copies(id, &pf.features, &places)
    }

    pub(crate) fn rebuild_mirror(
        &self,
        id: u32,
        m: &MirrorFeature,
        st: &mut BuildState,
    ) -> Result<(), String> {
        let p1 = self.resolve_plane(&m.plane, st)?;
        let r1 = (p1.origin, p1.z);
        let mut places = vec![Place::Reflect(r1.0, r1.1)];
        if let Some(p2) = &m.plane2 {
            let p2 = self.resolve_plane(p2, st)?;
            let r2 = (p2.origin, p2.z);
            if r1.1.cross(r2.1).length() < 1e-9 {
                return Err("the two mirror planes are parallel".into());
            }
            if m.seed_only && r1.1.dot(r2.1).abs() > 1e-9 {
                return Err("mirroring the seed only needs the two planes square to each other".into());
            }
            places.push(Place::Reflect(r2.0, r2.1));
            if !m.seed_only {
                places.push(Place::Twice(r1, r2));
            }
        }
        if !m.bodies {
            return st.place_copies(id, &m.features, &places);
        }
        // The whole body: each copy is joined to what is there.
        if st.body.is_empty() {
            return Err("there is no body to mirror".into());
        }
        let original = st.body.clone();
        let done = st.exact_step(id, |bodies| {
            let mut out = bodies.to_vec();
            for p in &places {
                for b in bodies {
                    out = exact::combine(&out, &p.exact(b)?, BodyOp::Boss)?;
                }
            }
            Ok(out)
        });
        if !done {
            for p in &places {
                st.body = st.body.union(&p.faceted(&original));
            }
        }
        Ok(())
    }

    pub(crate) fn rebuild_sketch_pattern(
        &self,
        id: u32,
        sp: &SketchPatternFeature,
        st: &mut BuildState,
    ) -> Result<(), String> {
        let sf = self.sketch(sp.sketch).ok_or("the pattern's sketch is missing")?;
        let frame = *st.frames.get(&sp.sketch).ok_or("the pattern's sketch failed or is suppressed")?;
        let points: Vec<Vec3> =
            crate::hole::positions(&sf.sketch).into_iter().map(|p| frame.to_world(p)).collect();
        if points.is_empty() {
            return Err("the sketch has no points to place copies at; add sketch points".into());
        }
        let reference = match &sp.reference {
            Some(r) => self.resolve_point(r, st)?,
            None => {
                let first = sp.features.first().ok_or("choose at least one feature to copy")?;
                let (tool, _) = st.tools.get(first).ok_or("a seed feature is missing or failed")?;
                tool.mass_properties().centroid
            }
        };
        let n = frame.z;
        let places: Vec<Place> = points
            .iter()
            .map(|q| {
                let v = *q - reference;
                v - n * v.dot(n)
            })
            .filter(|v| v.length() > 1e-9)
            .map(|v| Place::Move(translation(v)))
            .collect();
        st.place_copies(id, &sp.features, &places)
    }
}

#[cfg(test)]
mod tests {
    //! Statements from `docs/re/mirror-and-patterns.md`.
    use super::*;
    use anvil_kernel::v2;

    fn sketch(doc: &mut Document, plane: PlaneRef, f: impl FnOnce(&mut anvil_sketch::Sketch)) -> u32 {
        let mut sf = SketchFeature::new(plane);
        f(&mut sf.sketch);
        doc.add_feature(FeatureKind::Sketch(sf))
    }

    /// A 100 x 60 x 10 plate with its corner at the origin, and a 10 x 10 through cut
    /// near the corner, at (5..15, 5..15).
    fn plate_with_cut() -> (Document, u32) {
        let mut doc = Document::new();
        let s = sketch(&mut doc, PlaneRef::Front, |s| {
            s.add_rectangle(v2(0.0, 0.0), v2(100.0, 60.0));
        });
        let base = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature { sketch: s, ..Default::default() }));
        let top =
            PlaneRef::Face(anvil_kernel::FaceId { feature: base, index: anvil_kernel::FaceId::END_CAP });
        let c = sketch(&mut doc, top, |s| {
            s.add_rectangle(v2(5.0, 5.0), v2(15.0, 15.0));
        });
        let cut = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: c,
            end: EndCondition::ThroughAll,
            op: BodyOp::Cut,
            ..Default::default()
        }));
        (doc, cut)
    }

    fn volume(doc: &mut Document) -> (f64, BuildState) {
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        (st.body.mass_properties().volume, st)
    }

    #[test]
    fn grid_seed_only_and_skips() {
        let (mut doc, cut) = plate_with_cut();
        let p = doc.add_feature(FeatureKind::LinearPattern(LinearPatternFeature {
            features: vec![cut],
            direction: Axis::X,
            spacing: Dim::Value(25.0),
            count: Dim::Value(4.0),
            direction2: Some(PatternDirection {
                direction: Axis::Y,
                spacing: Dim::Value(20.0),
                count: Dim::Value(3.0),
                reverse: false,
            }),
            ..Default::default()
        }));
        // 4 x 3 = 12 holes of 1000 each.
        let (v, st) = volume(&mut doc);
        assert!((v - 48000.0).abs() < 1e-6, "{v}");
        assert!(st.exact.is_some());
        let set = |doc: &mut Document, f: &dyn Fn(&mut LinearPatternFeature)| {
            if let Some(FeatureKind::LinearPattern(x)) = doc.feature_mut(p).map(|f| &mut f.kind) {
                f(x);
            }
        };
        // Seed only: 4 + 3 - 1 = 6 holes.
        set(&mut doc, &|x| x.seed_only = true);
        assert!((volume(&mut doc).0 - 54000.0).abs() < 1e-6);
        // Skipping (2, 1) and (1, 3): 4 holes.
        set(&mut doc, &|x| x.skip = vec![[2, 1], [1, 3]]);
        assert!((volume(&mut doc).0 - 56000.0).abs() < 1e-6);
        // Reversed, the copies run off the plate: only the seed cuts.
        set(&mut doc, &|x| {
            x.seed_only = false;
            x.skip.clear();
            x.reverse = true;
            x.direction2 = None;
        });
        assert!((volume(&mut doc).0 - 59000.0).abs() < 1e-6);
    }

    #[test]
    fn circular_instance_and_equal_spacing() {
        // Four 10 x 10 cuts round the plate's centre (50, 30): the seed at (65..75, 25..35)
        // turned about the z axis through the centre.
        let mut doc = Document::new();
        let s = sketch(&mut doc, PlaneRef::Front, |s| {
            s.add_rectangle(v2(0.0, 0.0), v2(100.0, 60.0));
        });
        let base = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature { sketch: s, ..Default::default() }));
        let top =
            PlaneRef::Face(anvil_kernel::FaceId { feature: base, index: anvil_kernel::FaceId::END_CAP });
        let c = sketch(&mut doc, top, |s| {
            s.add_rectangle(v2(65.0, 25.0), v2(75.0, 35.0));
        });
        let cut = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: c,
            end: EndCondition::ThroughAll,
            op: BodyOp::Cut,
            ..Default::default()
        }));
        let axis = Axis::Line { origin: Vec3 { x: 50.0, y: 30.0, z: 0.0 }, dir: Vec3::Z };
        let p = doc.add_feature(FeatureKind::CircularPattern(CircularPatternFeature {
            features: vec![cut],
            axis,
            total_angle: Dim::Value(360.0),
            count: Dim::Value(4.0),
            ..Default::default()
        }));
        assert!((volume(&mut doc).0 - 56000.0).abs() < 1e-6);
        // Instance spacing of 90 degrees, 3 instances: the same places less one.
        if let Some(FeatureKind::CircularPattern(x)) = doc.feature_mut(p).map(|f| &mut f.kind) {
            x.equal_spacing = false;
            x.total_angle = Dim::Value(90.0);
            x.count = Dim::Value(3.0);
        }
        assert!((volume(&mut doc).0 - 57000.0).abs() < 1e-6);
        // Direction 2, symmetric: 3 + 3 - 1 = 5 places, but 180 degrees each way is
        // the same place, so 4 holes.
        if let Some(FeatureKind::CircularPattern(x)) = doc.feature_mut(p).map(|f| &mut f.kind) {
            x.direction2 = Some(CircularDirection2 {
                symmetric: true,
                angle: Dim::Value(0.0),
                count: Dim::Value(1.0),
                equal_spacing: true,
            });
            x.skip = vec![[2, 3]];
        }
        assert!((volume(&mut doc).0 - 56000.0).abs() < 1e-6);
    }

    #[test]
    fn mirror_a_boss_in_one_and_two_planes() {
        // A 10 x 10 x 10 block at (20..30, 10..20); mirrored in the Right plane (x = 0)
        // and then in the Top plane (y = 0) too.
        let mut doc = Document::new();
        let s = sketch(&mut doc, PlaneRef::Front, |s| {
            s.add_rectangle(v2(20.0, 10.0), v2(30.0, 20.0));
        });
        let b = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature { sketch: s, ..Default::default() }));
        let m = doc.add_feature(FeatureKind::Mirror(MirrorFeature {
            plane: PlaneRef::Right,
            features: vec![b],
            bodies: false,
            plane2: None,
            seed_only: false,
        }));
        assert_eq!(doc.features[2].name, "Mirror1");
        let (v, st) = volume(&mut doc);
        assert!((v - 2000.0).abs() < 1e-6);
        let bb = st.body.bounds();
        assert!((bb.min.x + 30.0).abs() < 1e-9 && (bb.max.x - 30.0).abs() < 1e-9);
        assert!(st.exact.as_ref().is_some_and(|b| b.len() == 2));
        let set = |doc: &mut Document, seed_only: bool| {
            if let Some(FeatureKind::Mirror(x)) = doc.feature_mut(m).map(|f| &mut f.kind) {
                x.plane2 = Some(PlaneRef::Top);
                x.seed_only = seed_only;
            }
        };
        set(&mut doc, false);
        assert!((volume(&mut doc).0 - 4000.0).abs() < 1e-6);
        set(&mut doc, true);
        assert!((volume(&mut doc).0 - 3000.0).abs() < 1e-6);
    }

    #[test]
    fn mirror_the_body_about_its_own_face() {
        // A filleted block mirrored about its end face joins into one body twice as long.
        let mut doc = Document::new();
        let s = sketch(&mut doc, PlaneRef::Front, |s| {
            s.add_rectangle(v2(0.0, 0.0), v2(20.0, 10.0));
        });
        let b = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature { sketch: s, ..Default::default() }));
        let right_face = PlaneRef::Face(anvil_kernel::FaceId::side(b, 1));
        doc.add_feature(FeatureKind::Mirror(MirrorFeature {
            plane: right_face,
            features: vec![],
            bodies: true,
            plane2: None,
            seed_only: false,
        }));
        let (v, st) = volume(&mut doc);
        assert!((v - 4000.0).abs() < 1e-6, "{v}");
        assert!(st.exact.as_ref().is_some_and(|b| b.len() == 1));
        assert!((st.body.bounds().max.x - 40.0).abs() < 1e-9);
    }

    #[test]
    fn sketch_driven_copies() {
        let (mut doc, cut) = plate_with_cut();
        // Copies of the hole centred at three more points; the seed's centre is (10, 10).
        let pts = sketch(&mut doc, PlaneRef::Front, |s| {
            for (x, y) in [(50.0, 30.0), (80.0, 40.0), (10.0, 10.0), (30.0, 50.0)] {
                s.add_point(v2(x, y));
            }
        });
        doc.add_feature(FeatureKind::SketchPattern(SketchPatternFeature {
            features: vec![cut],
            sketch: pts,
            reference: None,
        }));
        assert!((volume(&mut doc).0 - 56000.0).abs() < 1e-6);
    }
}
