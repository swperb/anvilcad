//! Revolves: one direction, mid-plane and two directions, and thin walls, built on both
//! kernels. The behaviour is specified in `docs/re/extrude-revolve.md` ("Revolve").
//!
//! A positive angle turns the profile towards the sketch normal, as revolves always
//! have here; Reverse turns it the other way. Mid-plane and two-direction revolves start
//! from the profile turned back about the axis, so the sketch sits inside the swept
//! angle.

use crate::document::*;
use crate::exact;
use crate::rebuild::BuildState;
use anvil_brep as brep;
use anvil_kernel::build::revolve;
use anvil_kernel::{Frame, Profile, Solid, Vec2, Vec3};
use anvil_sketch::Geom;
use std::f64::consts::TAU;

pub(crate) struct RevolvePlan {
    frame: Frame,
    profiles: Vec<Profile>,
    axis_point: Vec2,
    axis_dir: Vec2,
    /// Start angle (turning the profile back) and sweep, in radians.
    start: f64,
    sweep: f64,
}

/// `v` turned by `angle` about the unit vector `k`.
fn rotate(v: Vec3, k: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

impl Document {
    pub(crate) fn plan_revolve(&self, rf: &RevolveFeature, st: &BuildState) -> Result<RevolvePlan, String> {
        let num = |d: &Dim, what: &str| d.eval(&st.variables).map_err(|e| format!("{what}: {e}"));
        let sf = self.sketch(rf.sketch).ok_or("the sketch this feature uses is missing")?;
        let sk = &sf.sketch;
        let frame =
            *st.frames.get(&rf.sketch).ok_or("the sketch this feature uses failed or is suppressed")?;
        let (a, b) = match sk.entities.get(rf.axis).map(|e| e.geom) {
            Some(Geom::Line { a, b }) => (sk.pos(a), sk.pos(b)),
            _ => return Err("the axis of revolution must be a sketch line".into()),
        };
        if a.distance(b) < 1e-9 {
            return Err("the axis of revolution has no length".into());
        }
        let profiles = match &rf.thin {
            Some(w) => {
                // The axis is never part of the wall.
                let mut walls = sk.clone();
                if let Some(e) = walls.entities.get_mut(rf.axis) {
                    e.construction = true;
                }
                let fillet = w.fillet.as_ref().map(|f| num(f, "fillet radius")).transpose()?;
                crate::thin::wall_profiles(
                    &walls,
                    w,
                    num(&w.thickness, "thickness")?,
                    num(&w.thickness2, "second thickness")?,
                    fillet,
                )?
            }
            // Like SOLIDWORKS, an open profile may be closed along the axis.
            None => match sk.profiles() {
                Ok(p) => p,
                Err(e) => match crate::rebuild::close_along_axis(sk, a, b).map(|s| s.profiles()) {
                    Some(Ok(p)) => p,
                    _ => {
                        return Err(match e {
                            anvil_sketch::ProfileError::OpenContour { .. } => {
                                "open profile: close it along the axis, or use a thin feature".into()
                            }
                            e => e.to_string(),
                        })
                    }
                },
            },
        };
        let deg = num(&rf.angle, "angle")?;
        if !(deg > 0.0 && deg <= 360.0 + 1e-9) {
            return Err("the angle must be more than 0 and at most 360 degrees".into());
        }
        let s = if rf.reverse { -1.0 } else { 1.0 };
        let (start, sweep) = match &rf.kind {
            RevolveKind::OneDirection => (0.0, s * deg),
            RevolveKind::MidPlane => (-0.5 * deg, deg),
            RevolveKind::TwoDirection { angle2 } => {
                let d2 = num(angle2, "Direction 2 angle")?;
                if d2 <= 0.0 {
                    return Err("the Direction 2 angle must be more than 0".into());
                }
                if deg + d2 > 360.0 + 1e-9 {
                    return Err("the two directions together turn more than 360 degrees".into());
                }
                (-s * d2, s * (deg + d2))
            }
        };
        let (start, sweep) =
            if sweep.abs() >= 360.0 - 1e-9 { (0.0, 360.0 * sweep.signum()) } else { (start, sweep) };
        Ok(RevolvePlan {
            frame,
            profiles,
            axis_point: a,
            axis_dir: b - a,
            start: start.to_radians(),
            sweep: sweep.to_radians(),
        })
    }
}

impl RevolvePlan {
    /// The sketch frame turned `start` about the axis, the way a positive angle turns.
    fn start_frame(&self) -> Frame {
        if self.start == 0.0 {
            return self.frame;
        }
        let f = &self.frame;
        let d = self.axis_dir.normalized();
        let side = self
            .profiles
            .iter()
            .flat_map(|p| p.outer.polyline(0.01))
            .map(|p| d.cross(p - self.axis_point))
            .find(|s| s.abs() > 1e-9)
            .map_or(1.0, f64::signum);
        let radial = (f.x * (-d.y) + f.y * d.x) * side;
        let k = radial.cross(f.z).normalized();
        let o = f.to_world(self.axis_point);
        let turn = |v: Vec3| rotate(v, k, self.start);
        Frame { origin: o + turn(f.origin - o), x: turn(f.x), y: turn(f.y), z: turn(f.z) }
    }

    pub(crate) fn faceted(&self, id: u32) -> Result<Solid, String> {
        revolve(
            &self.profiles,
            &self.start_frame(),
            self.axis_point,
            self.axis_dir,
            self.sweep.clamp(-TAU, TAU),
            id,
        )
        .map_err(|e| e.to_string())
    }

    pub(crate) fn exact(&self, id: u32) -> Result<Vec<brep::Solid>, String> {
        exact::revolve(&self.profiles, &self.start_frame(), self.axis_point, self.axis_dir, self.sweep, id)
    }
}

#[cfg(test)]
mod tests {
    //! The revolve statements of `docs/re/extrude-revolve.md`, numbered as there.
    use super::*;
    use anvil_kernel::v2;
    use std::f64::consts::PI;

    /// A sketch on the Front plane with a centerline on the y axis and a closed
    /// rectangle, or an open chain when `open` is set.
    fn doc_with(rect: ((f64, f64), (f64, f64))) -> (Document, u32) {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let s = &mut sf.sketch;
        let p = s.add_point(v2(0.0, -50.0));
        let q = s.add_point(v2(0.0, 50.0));
        let axis = s.add_line(p, q);
        s.entities[axis].construction = true;
        s.add_rectangle(v2(rect.0 .0, rect.0 .1), v2(rect.1 .0, rect.1 .1));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let r =
            doc.add_feature(FeatureKind::Revolve(RevolveFeature { sketch: sk, axis, ..Default::default() }));
        (doc, r)
    }

    fn set(doc: &mut Document, id: u32, f: impl FnOnce(&mut RevolveFeature)) {
        if let Some(FeatureKind::Revolve(r)) = doc.feature_mut(id).map(|f| &mut f.kind) {
            f(r);
        }
    }

    fn volume(doc: &mut Document) -> f64 {
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        st.body.mass_properties().volume
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 2e-3 * b
    }

    #[test]
    fn s26_s27_tube_at_full_and_partial_turns() {
        let (mut doc, r) = doc_with(((10.0, 0.0), (20.0, 20.0)));
        assert!(close(volume(&mut doc), 6000.0 * PI));
        set(&mut doc, r, |r| r.angle = Dim::Value(90.0));
        assert!(close(volume(&mut doc), 1500.0 * PI));
        set(&mut doc, r, |r| r.kind = RevolveKind::MidPlane);
        let st = doc.rebuild();
        assert!(close(st.body.mass_properties().volume, 1500.0 * PI));
        // Half on each side of the sketch plane.
        let b = st.body.bounds();
        assert!((b.min.z + b.max.z).abs() < 1e-6, "{b:?}");
        set(&mut doc, r, |r| r.kind = RevolveKind::TwoDirection { angle2: Dim::Value(45.0) });
        assert!(close(volume(&mut doc), 2250.0 * PI));
        set(&mut doc, r, |r| {
            r.angle = Dim::Value(270.0);
            r.kind = RevolveKind::TwoDirection { angle2: Dim::Value(120.0) };
        });
        assert!(doc.rebuild().errors.contains_key(&r));
        set(&mut doc, r, |r| r.kind = RevolveKind::TwoDirection { angle2: Dim::Value(90.0) });
        assert!(close(volume(&mut doc), 6000.0 * PI));
    }

    #[test]
    fn reverse_turns_the_other_way() {
        let (mut doc, r) = doc_with(((10.0, 0.0), (20.0, 20.0)));
        set(&mut doc, r, |r| r.angle = Dim::Value(90.0));
        let up = doc.rebuild().body.bounds();
        set(&mut doc, r, |r| r.reverse = true);
        let down = doc.rebuild().body.bounds();
        assert!(up.min.z > -1e-6 && down.max.z < 1e-6, "{up:?} {down:?}");
    }

    #[test]
    fn s28_s29_on_and_across_the_axis() {
        let (mut doc, _) = doc_with(((0.0, 0.0), (10.0, 20.0)));
        assert!(close(volume(&mut doc), 2000.0 * PI));
        let (mut doc, r) = doc_with(((-5.0, 0.0), (10.0, 20.0)));
        assert!(doc.rebuild().errors.contains_key(&r));
    }

    #[test]
    fn s31_thin_revolve_of_a_line() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let s = &mut sf.sketch;
        let (p, q) = (s.add_point(v2(0.0, -50.0)), s.add_point(v2(0.0, 50.0)));
        let axis = s.add_line(p, q);
        s.entities[axis].construction = true;
        let (a, b) = (s.add_point(v2(10.0, 0.0)), s.add_point(v2(10.0, 20.0)));
        s.add_line(a, b);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let mut w = ThinWall::new(2.0);
        w.kind = ThinKind::MidPlane;
        doc.add_feature(FeatureKind::Revolve(RevolveFeature {
            sketch: sk,
            axis,
            thin: Some(w),
            ..Default::default()
        }));
        assert!(close(volume(&mut doc), 800.0 * PI));
        assert_eq!(doc.features[1].name, "Revolve-Thin1");
    }
}
