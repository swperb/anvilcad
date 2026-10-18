//! Swept bosses and cuts: a profile carried along a path, square to it. The behaviour is
//! specified in `docs/re/sweep-and-loft.md` ("Sweep").
//!
//! The path is the one chain of lines and arcs in its sketch, smooth at every joint. The
//! profile's plane must meet the path at a point where the path runs square through it;
//! from there the sweep follows the path one way, the other, or both. Along a line the
//! profile sweeps a prism and along an arc a solid of revolution, so the result is exact.

use crate::document::*;
use crate::exact;
use crate::rebuild::BuildState;
use crate::thin::chains;
use anvil_brep as brep;
use anvil_brep::sweep::PathSegment;
use anvil_kernel::{v2, Frame, Loop2, Profile, Seg2, Solid, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// Which way along the path a sweep goes from its profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SweepDirection {
    /// Towards the path's end (its last curve's far end).
    #[default]
    Direction1,
    /// Towards the path's start.
    Direction2,
    Bidirectional,
}

impl SweepDirection {
    pub const ALL: [SweepDirection; 3] =
        [SweepDirection::Direction1, SweepDirection::Direction2, SweepDirection::Bidirectional];
    pub fn label(&self) -> &'static str {
        match self {
            SweepDirection::Direction1 => "Direction 1",
            SweepDirection::Direction2 => "Direction 2",
            SweepDirection::Bidirectional => "Bidirectional",
        }
    }
}

/// A swept boss or cut.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SweepFeature {
    /// The sketch holding the profile; unused with a circular profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<u32>,
    /// The sketch holding the path.
    pub path: u32,
    /// A round profile of this diameter, square to the path at its start, instead of a
    /// profile sketch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<Dim>,
    pub op: BodyOp,
    #[serde(default)]
    pub direction: SweepDirection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thin: Option<ThinWall>,
}

impl Default for SweepFeature {
    fn default() -> Self {
        SweepFeature {
            profile: None,
            path: 0,
            diameter: None,
            op: BodyOp::Boss,
            direction: SweepDirection::Direction1,
            thin: None,
        }
    }
}

impl SweepFeature {
    pub(crate) fn dependencies(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.profile.into_iter().collect();
        v.push(self.path);
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// A run of path segments in model space.
type Path = Vec<PathSegment>;

fn reversed(path: &[PathSegment]) -> Path {
    path.iter()
        .rev()
        .map(|s| match *s {
            PathSegment::Line { a, b } => PathSegment::Line { a: b, b: a },
            PathSegment::Arc { center, axis, angle, .. } => {
                PathSegment::Arc { center, axis: -axis, start: s.end(), angle }
            }
        })
        .collect()
}

fn d(v: Vec3) -> glam::DVec3 {
    glam::DVec3::new(v.x, v.y, v.z)
}

fn k(v: glam::DVec3) -> Vec3 {
    Vec3 { x: v.x, y: v.y, z: v.z }
}

/// The path segment cut at a fraction `t` of its length: the part before and after.
fn split(s: &PathSegment, t: f64) -> (PathSegment, PathSegment) {
    match *s {
        PathSegment::Line { a, b } => {
            let m = a + (b - a) * t;
            (PathSegment::Line { a, b: m }, PathSegment::Line { a: m, b })
        }
        PathSegment::Arc { center, axis, start, angle } => {
            let m = brep::sweep::rotate(start, center, axis, angle * t);
            (
                PathSegment::Arc { center, axis, start, angle: angle * t },
                PathSegment::Arc { center, axis, start: m, angle: angle * (1.0 - t) },
            )
        }
    }
}

fn point_at(s: &PathSegment, t: f64) -> glam::DVec3 {
    match *s {
        PathSegment::Line { a, b } => a + (b - a) * t,
        PathSegment::Arc { center, axis, start, angle } => {
            brep::sweep::rotate(start, center, axis, angle * t)
        }
    }
}

/// The profile turned to face along `dir`: as it is, or flipped over (x mirrored) when
/// its normal points the other way.
fn facing(frame: &Frame, profiles: &[Profile], dir: Vec3) -> (Frame, Vec<Profile>) {
    if dir.dot(frame.z) >= 0.0 {
        return (*frame, profiles.to_vec());
    }
    let f = Frame { origin: frame.origin, x: -frame.x, y: frame.y, z: -frame.z };
    let mirror = |l: &Loop2| {
        Loop2::new(
            l.segs
                .iter()
                .map(|s| match *s {
                    Seg2::Line { a, b, source } => Seg2::Line { a: v2(-a.x, a.y), b: v2(-b.x, b.y), source },
                    Seg2::Arc { center, radius, start, sweep, source } => Seg2::Arc {
                        center: v2(-center.x, center.y),
                        radius,
                        start: std::f64::consts::PI - start,
                        sweep: -sweep,
                        source,
                    },
                })
                .collect(),
        )
    };
    let p = profiles
        .iter()
        .map(|p| Profile::new(mirror(&p.outer), p.holes.iter().map(mirror).collect()))
        .collect();
    (f, p)
}

pub(crate) struct SweepPlan {
    /// Each run: the profile placed at its start, facing along it, and the path.
    runs: Vec<(Frame, Vec<Profile>, Path)>,
}

impl Document {
    pub(crate) fn plan_sweep(&self, sw: &SweepFeature, st: &BuildState) -> Result<SweepPlan, String> {
        let num = |d: &Dim, what: &str| d.eval(&st.variables).map_err(|e| format!("{what}: {e}"));
        let ps = self.sketch(sw.path).ok_or("the path's sketch is missing")?;
        let pf = *st.frames.get(&sw.path).ok_or("the path's sketch failed or is suppressed")?;
        let mut cs = chains(&ps.sketch)?;
        if cs.len() != 1 {
            return Err("the path sketch must hold exactly one chain of curves".into());
        }
        let chain = cs.remove(0);
        let path: Path = chain
            .segs
            .iter()
            .map(|s| match *s {
                Seg2::Line { a, b, .. } => PathSegment::Line { a: d(pf.to_world(a)), b: d(pf.to_world(b)) },
                Seg2::Arc { center, radius, start, sweep, .. } => PathSegment::Arc {
                    center: d(pf.to_world(center)),
                    axis: d(pf.z * sweep.signum()),
                    start: d(pf.to_world(center + Vec2::from_angle(start) * radius)),
                    angle: sweep.abs(),
                },
            })
            .collect();
        let first = path.first().ok_or("the path is empty")?;

        // The profile and its plane.
        let (frame, profiles) = match &sw.diameter {
            Some(dia) => {
                let r = num(dia, "diameter")? / 2.0;
                if r <= 0.0 {
                    return Err("the diameter must be greater than zero".into());
                }
                let t = k(first.tangent(false));
                let f = Frame::from_normal(k(first.start()), t, t.any_perpendicular());
                (f, vec![Profile::new(Loop2::circle(Vec2::ZERO, r, 0), vec![])])
            }
            None => {
                let id = sw.profile.ok_or("choose the profile's sketch")?;
                let sf = self.sketch(id).ok_or("the profile's sketch is missing")?;
                let f = *st.frames.get(&id).ok_or("the profile's sketch failed or is suppressed")?;
                let p = match &sw.thin {
                    Some(w) => {
                        let fillet = w.fillet.as_ref().map(|f| num(f, "fillet radius")).transpose()?;
                        crate::thin::wall_profiles(
                            &sf.sketch,
                            w,
                            num(&w.thickness, "thickness")?,
                            num(&w.thickness2, "second thickness")?,
                            fillet,
                        )?
                    }
                    None => sf.sketch.profiles().map_err(|e| match e {
                        anvil_sketch::ProfileError::OpenContour { .. } => {
                            "the profile must be closed".to_string()
                        }
                        e => e.to_string(),
                    })?,
                };
                (f, p)
            }
        };

        // Where the path passes through the profile's plane.
        let side = |p: glam::DVec3| (k(p) - frame.origin).dot(frame.z);
        let size = path.iter().map(PathSegment::length).sum::<f64>().max(1.0);
        let tol = 1e-6 * size;
        let (before, after): (Path, Path) = if side(first.start()).abs() < tol {
            (Vec::new(), path.clone())
        } else if path.last().is_some_and(|s| side(s.end()).abs() < tol) {
            (path.clone(), Vec::new())
        } else {
            let mut found = None;
            'seg: for (i, s) in path.iter().enumerate() {
                let n = 64;
                for j in 0..n {
                    let (t0, t1) = (j as f64 / n as f64, (j + 1) as f64 / n as f64);
                    let (f0, f1) = (side(point_at(s, t0)), side(point_at(s, t1)));
                    if f0 == 0.0 || f0.signum() != f1.signum() {
                        let (mut lo, mut hi) = (t0, t1);
                        for _ in 0..80 {
                            let m = 0.5 * (lo + hi);
                            if side(point_at(s, m)).signum() == f0.signum() && f0 != 0.0 {
                                lo = m;
                            } else {
                                hi = m;
                            }
                        }
                        found = Some((i, 0.5 * (lo + hi)));
                        break 'seg;
                    }
                }
            }
            let (i, t) = found.ok_or("the path does not pass through the profile's plane")?;
            let (a, b) = split(&path[i], t);
            let mut before: Path = path[..i].to_vec();
            before.push(a);
            let mut after = vec![b];
            after.extend_from_slice(&path[i + 1..]);
            (before, after)
        };
        let meet =
            after.first().map(|s| s.start()).or(before.last().map(|s| s.end())).ok_or("the path is empty")?;
        if (k(meet) - frame.origin).dot(frame.z).abs() > tol {
            return Err("the path must meet the profile's plane".into());
        }
        // The profile stays where it was drawn; the path only fixes where it starts.
        let start_frame = Frame { origin: k(meet), ..frame };
        let shift = |p: &Profile| {
            let o = frame.to_local(k(meet));
            let mv = |l: &Loop2| {
                Loop2::new(
                    l.segs
                        .iter()
                        .map(|s| match *s {
                            Seg2::Line { a, b, source } => Seg2::Line { a: a - o, b: b - o, source },
                            Seg2::Arc { center, radius, start, sweep, source } => {
                                Seg2::Arc { center: center - o, radius, start, sweep, source }
                            }
                        })
                        .collect(),
                )
            };
            Profile::new(mv(&p.outer), p.holes.iter().map(mv).collect())
        };
        let profiles: Vec<Profile> = profiles.iter().map(shift).collect();

        let mut runs = Vec::new();
        let forward = !after.is_empty();
        let backward = !before.is_empty();
        let want1 = matches!(sw.direction, SweepDirection::Direction1 | SweepDirection::Bidirectional);
        let want2 = matches!(sw.direction, SweepDirection::Direction2 | SweepDirection::Bidirectional);
        // A profile at an end of the path sweeps along it, whichever direction is set.
        let (go1, go2) = match (forward, backward) {
            (true, false) => (true, false),
            (false, true) => (false, true),
            _ => (want1, want2),
        };
        for (go, run) in [(go1, after), (go2, reversed(&before))] {
            if !go || run.is_empty() {
                continue;
            }
            let t = k(run[0].tangent(false));
            if t.cross(frame.z).length() > 1e-6 {
                return Err("the profile's plane is not square to the path where they meet".into());
            }
            let (f, p) = facing(&start_frame, &profiles, t);
            runs.push((f, p, run));
        }
        if runs.is_empty() {
            return Err("the sweep has no length in the chosen direction".into());
        }
        Ok(SweepPlan { runs })
    }
}

impl SweepPlan {
    pub(crate) fn faceted(&self, id: u32) -> Result<Solid, String> {
        let mut out: Option<Solid> = None;
        for (frame, profiles, path) in &self.runs {
            let mut f = *frame;
            for seg in path {
                let piece = match *seg {
                    PathSegment::Line { a, b } => {
                        let len = (b - a).length();
                        let s = anvil_kernel::build::extrude(profiles, &f, 0.0, len, id)
                            .map_err(|e| e.to_string())?;
                        f = Frame { origin: f.origin + f.z * len, ..f };
                        s
                    }
                    PathSegment::Arc { center, axis, angle, .. } => {
                        let c = f.to_local(k(center));
                        let ax = k(axis);
                        let dir = v2(ax.dot(f.x), ax.dot(f.y));
                        let s =
                            anvil_kernel::build::revolve(profiles, &f, v2(c.x, c.y), dir, angle.abs(), id)
                                .map_err(|e| e.to_string())?;
                        let turn = |v: Vec3| k(brep::sweep::rotate(d(v), glam::DVec3::ZERO, axis, angle));
                        f = Frame {
                            origin: k(brep::sweep::rotate(d(f.origin), center, axis, angle)),
                            x: turn(f.x),
                            y: turn(f.y),
                            z: turn(f.z),
                        };
                        s
                    }
                };
                out = Some(match out {
                    None => piece,
                    Some(o) => o.union(&piece),
                });
            }
        }
        out.ok_or_else(|| "the sweep has no length".into())
    }

    pub(crate) fn exact(&self, id: u32) -> Result<Vec<brep::Solid>, String> {
        let mut out: Vec<brep::Solid> = Vec::new();
        for (frame, profiles, path) in &self.runs {
            for region in exact::regions(profiles)? {
                for mut s in
                    brep::sweep::sweep(&exact::frame(frame), &region, path).map_err(|e| e.to_string())?
                {
                    s.set_owner(id);
                    out = if out.is_empty() { vec![s] } else { exact::combine(&out, &s, BodyOp::Boss)? };
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    //! Sweep statements from `docs/re/sweep-and-loft.md`, numbered as there.
    use super::*;
    use std::f64::consts::PI;

    /// A path sketch on the Top plane (its x is world x, its y world -z).
    fn path_doc(f: impl FnOnce(&mut anvil_sketch::Sketch)) -> (Document, u32) {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        f(&mut sf.sketch);
        let p = doc.add_feature(FeatureKind::Sketch(sf));
        (doc, p)
    }

    /// A circle of diameter `dia` centred at `c` on the Right plane (normal +x; its
    /// sketch x is world -z and y is world y).
    fn circle_on_right(doc: &mut Document, c: (f64, f64), dia: f64) -> u32 {
        let mut sf = SketchFeature::new(PlaneRef::Right);
        let p = sf.sketch.add_point(v2(c.0, c.1));
        sf.sketch.add_circle(p, dia / 2.0);
        doc.add_feature(FeatureKind::Sketch(sf))
    }

    fn sweep(doc: &mut Document, profile: Option<u32>, path: u32, f: SweepFeature) -> u32 {
        doc.add_feature(FeatureKind::Sweep(SweepFeature { profile, path, ..f }))
    }

    fn volume(doc: &mut Document) -> (f64, BuildState) {
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        (st.body.mass_properties().volume, st)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 3e-3 * b
    }

    /// Line (0,0) -> (len,0) along x on the Front plane.
    fn line(s: &mut anvil_sketch::Sketch, a: (f64, f64), b: (f64, f64)) {
        let (p, q) = (s.add_point(v2(a.0, a.1)), s.add_point(v2(b.0, b.1)));
        s.add_line(p, q);
    }

    #[test]
    fn s01_straight_sweep_is_a_cylinder() {
        let (mut doc, p) = path_doc(|s| line(s, (0.0, 0.0), (50.0, 0.0)));
        let c = circle_on_right(&mut doc, (0.0, 0.0), 10.0);
        sweep(&mut doc, Some(c), p, SweepFeature::default());
        let (v, st) = volume(&mut doc);
        assert!(close(v, 1250.0 * PI), "{v}");
        assert!(st.exact.is_some());
        assert_eq!(doc.features[2].name, "Sweep1");
    }

    #[test]
    fn s02_s03_arc_and_line_arc_line() {
        // A quarter arc of radius 20 from (0,0) heading +x, centre (0,20): the path
        // starts square to the Right plane.
        let (mut doc, p) = path_doc(|s| {
            let a = s.add_point(v2(0.0, 0.0));
            let b = s.add_point(v2(20.0, 20.0));
            let c = s.add_point(v2(0.0, 20.0));
            s.add_arc(c, a, b);
        });
        let c = circle_on_right(&mut doc, (0.0, 0.0), 4.0);
        sweep(&mut doc, Some(c), p, SweepFeature::default());
        assert!(close(volume(&mut doc).0, 40.0 * PI * PI));

        let (mut doc, p) = path_doc(|s| {
            let a = s.add_point(v2(0.0, 0.0));
            let b = s.add_point(v2(30.0, 0.0));
            let c = s.add_point(v2(30.0, 20.0));
            let e = s.add_point(v2(50.0, 20.0));
            let f = s.add_point(v2(50.0, 50.0));
            s.add_line(a, b);
            s.add_arc(c, b, e);
            s.add_line(e, f);
        });
        let c = circle_on_right(&mut doc, (0.0, 0.0), 4.0);
        sweep(&mut doc, Some(c), p, SweepFeature::default());
        let (v, st) = volume(&mut doc);
        assert!(close(v, 240.0 * PI + 40.0 * PI * PI), "{v}");
        assert!(st.exact.is_some());
    }

    #[test]
    fn s04_closed_circle_path_is_a_torus() {
        let (mut doc, p) = path_doc(|s| {
            let c = s.add_point(v2(0.0, 20.0));
            s.add_circle(c, 20.0);
        });
        // The circle path starts at angle 0, (20, 20), heading +y: profile on a plane
        // through (20, 20) facing +y.
        let base = doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base: PlaneRef::Top,
            offset: Dim::Value(20.0),
            ..Default::default()
        }));
        let mut sf = SketchFeature::new(PlaneRef::Feature(base));
        let q = sf.sketch.add_point(v2(20.0, 0.0));
        sf.sketch.add_circle(q, 2.0);
        let c = doc.add_feature(FeatureKind::Sketch(sf));
        sweep(&mut doc, Some(c), p, SweepFeature::default());
        assert!(close(volume(&mut doc).0, 160.0 * PI * PI));
    }

    #[test]
    fn s11_s18_circular_profile_and_both_ways() {
        let (mut doc, p) = path_doc(|s| line(s, (0.0, 0.0), (40.0, 0.0)));
        sweep(&mut doc, None, p, SweepFeature { diameter: Some(Dim::Value(6.0)), ..Default::default() });
        assert!(close(volume(&mut doc).0, 360.0 * PI));

        // A profile at the middle of a 60 long path.
        let (mut doc, p) = path_doc(|s| line(s, (-30.0, 0.0), (30.0, 0.0)));
        let c = circle_on_right(&mut doc, (0.0, 0.0), 10.0);
        let s = sweep(&mut doc, Some(c), p, SweepFeature::default());
        assert!(close(volume(&mut doc).0, 750.0 * PI));
        if let Some(FeatureKind::Sweep(x)) = doc.feature_mut(s).map(|f| &mut f.kind) {
            x.direction = SweepDirection::Bidirectional;
        }
        let (v, st) = volume(&mut doc);
        assert!(close(v, 1500.0 * PI), "{v}");
        assert!(st.body.bounds().min.x < -29.9 && st.body.bounds().max.x > 29.9);
    }

    #[test]
    fn s12_thin_sweeps() {
        for (kind, reverse, t, vol) in [
            (ThinKind::MidPlane, false, 2.0, 1000.0 * PI),
            (ThinKind::OneDirection, false, 1.0, 550.0 * PI),
            (ThinKind::OneDirection, true, 1.0, 450.0 * PI),
        ] {
            let (mut doc, p) = path_doc(|s| line(s, (0.0, 0.0), (50.0, 0.0)));
            let c = circle_on_right(&mut doc, (0.0, 0.0), 10.0);
            let mut w = ThinWall::new(t);
            w.kind = kind;
            w.reverse = reverse;
            sweep(&mut doc, Some(c), p, SweepFeature { thin: Some(w), ..Default::default() });
            assert!(close(volume(&mut doc).0, vol));
        }
    }

    #[test]
    fn s16_swept_cut_and_s19_errors() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sf.sketch.add_rectangle(v2(0.0, -50.0), v2(100.0, 50.0));
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: s,
            depth: Dim::Value(20.0),
            ..Default::default()
        }));
        let mid = doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base: PlaneRef::Front,
            offset: Dim::Value(10.0),
            ..Default::default()
        }));
        let mut pf = SketchFeature::new(PlaneRef::Feature(mid));
        line(&mut pf.sketch, (0.0, 0.0), (100.0, 0.0));
        let p = doc.add_feature(FeatureKind::Sketch(pf));
        // On the Right plane, the path's start (0, 0, 10) is at sketch (-10, 0).
        let c = circle_on_right(&mut doc, (-10.0, 0.0), 4.0);
        sweep(&mut doc, Some(c), p, SweepFeature { op: BodyOp::Cut, ..Default::default() });
        assert_eq!(doc.features.last().map(|f| f.name.as_str()), Some("Cut-Sweep1"));
        assert!(close(volume(&mut doc).0, 200000.0 - 400.0 * PI));

        // A 2 mm radius profile on an arc of radius 1.5 runs into itself.
        let (mut doc, p) = path_doc(|s| {
            let a = s.add_point(v2(0.0, 0.0));
            let b = s.add_point(v2(1.5, 1.5));
            let c = s.add_point(v2(0.0, 1.5));
            s.add_arc(c, a, b);
        });
        let c = circle_on_right(&mut doc, (0.0, 0.0), 4.0);
        let bad = sweep(&mut doc, Some(c), p, SweepFeature::default());
        assert!(doc.rebuild().errors.contains_key(&bad));
    }
}
