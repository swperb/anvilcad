//! Draft and dome features, built on the exact kernel. The behaviour is specified in
//! `docs/re/rib-draft-dome.md`.

use crate::document::*;
use crate::exact;
use crate::failure::Cause;
use crate::rebuild::BuildState;
use crate::refgeom::{num_text, value};
use anvil_brep::KernelError;
use anvil_kernel::FaceId;
use serde::{Deserialize, Serialize};

/// Which faces besides the chosen ones a draft takes, by the neutral face's loops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Propagation {
    #[default]
    None,
    /// Faces along the neutral face's outer loop.
    Outer,
    /// Faces along its inner loops.
    Inner,
    /// Faces along every loop.
    All,
}

/// Neutral-plane draft: faces turned about where they meet the neutral plane so they
/// lean `angle` degrees from the direction of pull (the plane's normal). Without
/// `reverse` the body narrows in the direction of pull.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftFeature {
    pub neutral: PlaneRef,
    #[serde(default)]
    pub faces: Vec<FaceId>,
    pub angle: Dim,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub propagation: Propagation,
}

/// A dome on flat round faces: a spherical cap of `height` (0: the hemisphere tangent
/// to the cylinder below), cut in when `reverse` is set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomeFeature {
    pub faces: Vec<FaceId>,
    pub height: Dim,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub elliptical: bool,
}

impl DraftFeature {
    pub(crate) fn dependencies(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.faces.iter().map(|f| f.feature).collect();
        v.extend(self.neutral.dependency());
        v.sort_unstable();
        v.dedup();
        v
    }
}

impl DomeFeature {
    pub(crate) fn dependencies(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.faces.iter().map(|f| f.feature).collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

const EXACT_ONLY: &str =
    "this feature needs the exact kernel, and the part is faceted from an earlier feature on";

impl Document {
    pub(crate) fn rebuild_draft(&self, id: u32, df: &DraftFeature, st: &mut BuildState) -> Result<(), Cause> {
        let angle = value(&df.angle, &st.variables, "draft angle")?;
        if !(angle > 0.0 && angle < 90.0) {
            return Err(Cause::told(
                format!("the draft angle ({} degrees) must be between 0 and 90 degrees", num_text(angle)),
                "Give it an angle above 0 and below 90 degrees; to lean the other way, turn on Reverse direction.",
            ));
        }
        let neutral = exact::frame(&self.resolve_plane(&df.neutral, st)?);
        let neutral_face = match df.neutral {
            PlaneRef::Face(f) => Some(f),
            _ => None,
        };
        if df.propagation != Propagation::None && neutral_face.is_none() {
            return Err(Cause::told(
                "propagating along loops needs a face of the part as the neutral plane",
                "Pick a flat face of the part as the neutral plane, or turn off Propagation.",
            ));
        }
        let bodies = st.exact.clone().ok_or_else(|| self.exact_only("drafts", st))?;
        let a = if df.reverse { -angle } else { angle }.to_radians();
        let mut out = Vec::new();
        let mut drafted = 0;
        for b in &bodies {
            let mut faces: Vec<usize> =
                (0..b.faces.len()).filter(|&i| df.faces.contains(&exact::face_id(&b.faces[i]))).collect();
            if let (Some(nf), true) = (neutral_face, df.propagation != Propagation::None) {
                if let Some(ni) = b.faces.iter().position(|f| exact::face_id(f) == nf) {
                    let face = &b.faces[ni];
                    let loops: Vec<&anvil_brep::topo::Loop> = match df.propagation {
                        Propagation::Outer => vec![&face.outer],
                        Propagation::Inner => face.inners.iter().collect(),
                        _ => face.loops().collect(),
                    };
                    for l in loops {
                        for c in &l.coedges {
                            for g in b.edge_faces(c.edge) {
                                if g.index() != ni && !faces.contains(&g.index()) {
                                    faces.push(g.index());
                                }
                            }
                        }
                    }
                }
            }
            if faces.is_empty() {
                out.push(b.clone());
                continue;
            }
            drafted += faces.len();
            out.push(anvil_brep::draft::draft_faces(b, &faces, &neutral, a).map_err(|e| match e {
                // The draft's own refusals are written for the user: say them as they are.
                KernelError::Unsupported(m) | KernelError::InvalidInput(m) => Cause::told(
                    m,
                    "Try a smaller angle, or draft other faces: flat faces square to the neutral plane, \
                     and round faces whose axis runs along the pull.",
                ),
                e => e.into(),
            })?);
        }
        if drafted == 0 {
            return Err(if df.faces.is_empty() && df.propagation == Propagation::None {
                Cause::told("it has no faces to draft", "Edit it and pick the faces to draft.")
            } else if df.faces.is_empty() {
                Cause::told(
                    "no faces run along the neutral face's loops it propagates to",
                    "Pick faces to draft, or choose other loops under Propagation.",
                )
            } else {
                Cause::told(
                    "the faces it drafts are no longer on the part",
                    "Edit it and pick the faces to draft again.",
                )
            });
        }
        if !st.exact_step(id, |_| Ok(out)) {
            return Err(st
                .faceted_from
                .as_ref()
                .map(|f| Cause::from(f.1.clone()))
                .unwrap_or_else(|| self.exact_only("drafts", st)));
        }
        Ok(())
    }

    pub(crate) fn rebuild_dome(&self, id: u32, d: &DomeFeature, st: &mut BuildState) -> Result<(), Cause> {
        let h = value(&d.height, &st.variables, "height")?;
        if h < 0.0 {
            return Err(Cause::told(
                format!("the height ({} mm) must not be negative", num_text(h)),
                "Give it a height of 0 or more; 0 makes a half sphere, and Reverse cuts the dome in.",
            ));
        }
        if d.faces.is_empty() {
            return Err(Cause::told("it has no face to dome", "Edit it and pick a flat round face."));
        }
        // Exactly, when the part is exact and the dome is a spherical cap.
        let exact_bodies = st.exact.clone();
        let done = st.exact_step(id, |_| {
            let mut bodies = exact_bodies.ok_or(EXACT_ONLY)?;
            for f in &d.faces {
                let bi = bodies
                    .iter()
                    .position(|b| b.faces.iter().any(|x| exact::face_id(x) == *f))
                    .ok_or("a face to dome no longer exists")?;
                let fi =
                    bodies[bi].faces.iter().position(|x| exact::face_id(x) == *f).ok_or("face missing")?;
                let mut result = anvil_brep::dome::dome(&bodies[bi], fi, h, d.elliptical, d.reverse)
                    .map_err(|e| e.to_string())?;
                for s in &mut result {
                    for face in &mut s.faces {
                        if face.owner == 0 {
                            face.owner = id;
                        }
                    }
                }
                bodies.splice(bi..=bi, result);
            }
            Ok(bodies)
        });
        if done {
            return Ok(());
        }
        // Otherwise (a half spheroid, which the exact kernel has no surface for, or a
        // faceted part) on the faceted kernel.
        for f in &d.faces {
            let tool = faceted_dome(&st.body, *f, h, d.elliptical, d.reverse, id)?;
            st.body = if d.reverse { st.body.subtract(&tool) } else { st.body.union(&tool) };
        }
        Ok(())
    }
}

/// A dome as a faceted solid on a flat round face: its section (the cap's outline from
/// the rim to the axis) turned about the face's axis, standing on the face (or hanging
/// below it, to be cut away, when reversed).
fn faceted_dome(
    body: &anvil_kernel::Solid,
    f: FaceId,
    h: f64,
    elliptical: bool,
    reverse: bool,
    id: u32,
) -> Result<anvil_kernel::Solid, Cause> {
    use anvil_kernel::{v2, Frame, Loop2, Profile};
    let gone =
        || Cause::told("the face to dome is no longer on the part", "Edit it and pick the face again.");
    let frame = body.planar_face_frame(f).ok_or_else(|| {
        Cause::told(
            "the face to dome is not flat, or is no longer on the part",
            "Edit it and pick a flat round face.",
        )
    })?;
    let (mut area, mut c) = (0.0, anvil_kernel::Vec3::ZERO);
    let mut verts = Vec::new();
    for p in body.face_polygons(f) {
        let a = p.area();
        area += a;
        c += p.verts.iter().fold(anvil_kernel::Vec3::ZERO, |s, v| s + *v) * (a / p.verts.len() as f64);
        verts.extend(p.verts.iter().copied());
    }
    if area <= 0.0 {
        return Err(gone());
    }
    let c = c * (1.0 / area);
    let r = verts.iter().map(|v| (*v - c).length()).fold(0.0, f64::max);
    if r <= 0.0 {
        return Err(gone());
    }
    // Height 0 is the dome tangent to the cylinder below: a hemisphere.
    let h = if h == 0.0 { r } else { h };
    let up = if reverse { -frame.z } else { frame.z };
    let u = frame.x;
    // The section in a plane through the axis: x out from the axis, y up the axis.
    let sec = Frame { origin: c, x: u, y: up, z: u.cross(up) };
    let n = 96;
    let mut pts = vec![v2(0.0, 0.0), v2(r, 0.0)];
    for k in 1..n {
        let t = std::f64::consts::FRAC_PI_2 * k as f64 / n as f64;
        let p = if elliptical {
            v2(r * t.cos(), h * t.sin())
        } else {
            // On the sphere through the rim with its top at height h.
            let big = (r * r + h * h) / (2.0 * h);
            let a0 = ((h - big) / big).asin();
            let a = a0 + (std::f64::consts::FRAC_PI_2 - a0) * k as f64 / n as f64;
            v2(big * a.cos(), (h - big) + big * a.sin())
        };
        pts.push(p);
    }
    pts.push(v2(0.0, h));
    let profile = Profile::new(Loop2::polygon(&pts, 0), vec![]);
    Ok(anvil_kernel::build::revolve(&[profile], &sec, v2(0.0, 0.0), v2(0.0, 1.0), std::f64::consts::TAU, id)?)
}

#[cfg(test)]
mod tests {
    //! Statements 10, 11, 16, 19 and 20 of `docs/re/rib-draft-dome.md`.
    use super::*;
    use anvil_kernel::v2;

    fn exact_volume(st: &BuildState) -> f64 {
        let p = anvil_brep::tess::TessParams { chord: 1e-4, max_angle: 0.01 };
        st.exact
            .as_ref()
            .map(|bs| {
                bs.iter()
                    .map(|b| {
                        anvil_brep::mass::properties(&anvil_brep::tess::tessellate(b, &p).unwrap()).volume
                    })
                    .sum()
            })
            .unwrap_or(0.0)
    }

    fn boxed(doc: &mut Document, a: (f64, f64), b: (f64, f64), plane: PlaneRef, depth: f64) -> u32 {
        let mut sf = SketchFeature::new(plane);
        sf.sketch.add_rectangle(v2(a.0, a.1), v2(b.0, b.1));
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: s,
            depth: Dim::Value(depth),
            ..Default::default()
        }))
    }

    #[test]
    fn draft_box_sides_both_ways() {
        let mut doc = Document::new();
        let e = boxed(&mut doc, (0.0, 0.0), (20.0, 20.0), PlaneRef::Front, 10.0);
        let d = doc.add_feature(FeatureKind::Draft(DraftFeature {
            neutral: PlaneRef::Front,
            faces: (0..4).map(|k| FaceId::side(e, k)).collect(),
            angle: Dim::Value(5.0),
            reverse: false,
            propagation: Propagation::None,
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!((exact_volume(&st) - 3660.251034).abs() < 1e-6 * 3660.0);
        if let Some(FeatureKind::Draft(x)) = doc.feature_mut(d).map(|f| &mut f.kind) {
            x.reverse = true;
        }
        let st = doc.rebuild();
        assert!((exact_volume(&st) - 4360.160342).abs() < 1e-6 * 4360.0);
    }

    #[test]
    fn draft_propagates_from_the_neutral_face() {
        let mut doc = Document::new();
        let base = boxed(&mut doc, (0.0, 0.0), (40.0, 40.0), PlaneRef::Front, 10.0);
        let top = PlaneRef::Face(FaceId { feature: base, index: FaceId::END_CAP });
        boxed(&mut doc, (15.0, 15.0), (25.0, 25.0), top, 5.0);
        let d = doc.add_feature(FeatureKind::Draft(DraftFeature {
            neutral: top,
            faces: vec![],
            angle: Dim::Value(5.0),
            reverse: false,
            propagation: Propagation::Inner,
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!((exact_volume(&st) - 16457.531379).abs() < 1e-5 * 16457.0, "{}", exact_volume(&st));
        if let Some(FeatureKind::Draft(x)) = doc.feature_mut(d).map(|f| &mut f.kind) {
            x.neutral = PlaneRef::Front;
        }
        assert!(doc.rebuild().errors.contains_key(&d));
    }

    #[test]
    fn elliptical_dome_on_the_faceted_kernel() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        sf.sketch.add_circle(c, 10.0);
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        let e = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: s,
            depth: Dim::Value(20.0),
            ..Default::default()
        }));
        doc.add_feature(FeatureKind::Dome(DomeFeature {
            faces: vec![FaceId { feature: e, index: FaceId::END_CAP }],
            height: Dim::Value(5.0),
            reverse: false,
            elliptical: true,
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        // Statement 22: 2000 pi + 2/3 pi 100 5 = 7330.382858, faceted.
        let v = st.body.mass_properties().volume;
        assert!((v - 7330.382858).abs() < 2e-3 * 7330.0, "{v}");
        assert!(st.exact.is_none());
    }

    #[test]
    fn dome_on_a_cylinder() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        sf.sketch.add_circle(c, 10.0);
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        let e = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: s,
            depth: Dim::Value(20.0),
            ..Default::default()
        }));
        let d = doc.add_feature(FeatureKind::Dome(DomeFeature {
            faces: vec![FaceId { feature: e, index: FaceId::END_CAP }],
            height: Dim::Value(5.0),
            reverse: false,
            elliptical: false,
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!((exact_volume(&st) - 7134.033318).abs() < 3e-5 * 7134.0, "{}", exact_volume(&st));
        if let Some(FeatureKind::Dome(x)) = doc.feature_mut(d).map(|f| &mut f.kind) {
            x.reverse = true;
        }
        let st = doc.rebuild();
        assert!((exact_volume(&st) - 5432.337297).abs() < 3e-5 * 5432.0);
    }

    fn told(c: &str, s: &str) -> (String, String) {
        (c.to_string(), s.to_string())
    }

    /// Makes the failing feature from the id of the body it works on.
    type Make = Box<dyn Fn(u32) -> FeatureKind>;

    /// A 10 mm high round boss of radius 10 on the Front plane; returns its extrude.
    fn cylinder(doc: &mut Document) -> u32 {
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        sf.sketch.add_circle(c, 10.0);
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: s,
            depth: Dim::Value(10.0),
            ..Default::default()
        }))
    }

    #[test]
    fn draft_failures_say_what_to_change() {
        let draft = |e: u32, f: &dyn Fn(&mut DraftFeature)| {
            let mut d = DraftFeature {
                neutral: PlaneRef::Front,
                faces: (0..4).map(|k| FaceId::side(e, k)).collect(),
                angle: Dim::Value(5.0),
                reverse: false,
                propagation: Propagation::None,
            };
            f(&mut d);
            FeatureKind::Draft(d)
        };
        let cases: Vec<(Make, (String, String))> = vec![
            (
                Box::new(move |e| draft(e, &|d| d.angle = Dim::Value(0.0))),
                told(
                    "the draft angle (0 degrees) must be between 0 and 90 degrees",
                    "Give it an angle above 0 and below 90 degrees; to lean the other way, turn on Reverse direction.",
                ),
            ),
            (
                Box::new(move |e| draft(e, &|d| d.angle = Dim::Expr("tilt".into()))),
                told(
                    "the draft angle cannot be worked out: unknown variable 'tilt'",
                    "Edit the draft angle and fix its equation, or define the variables it uses in Equations.",
                ),
            ),
            (
                Box::new(move |e| draft(e, &|d| d.propagation = Propagation::Outer)),
                told(
                    "propagating along loops needs a face of the part as the neutral plane",
                    "Pick a flat face of the part as the neutral plane, or turn off Propagation.",
                ),
            ),
            (
                Box::new(move |e| draft(e, &|d| d.faces.clear())),
                told("it has no faces to draft", "Edit it and pick the faces to draft."),
            ),
            (
                // The box's top has no inner loops to propagate along.
                Box::new(move |e| {
                    draft(e, &|d| {
                        d.neutral = PlaneRef::Face(FaceId { feature: e, index: FaceId::END_CAP });
                        d.faces.clear();
                        d.propagation = Propagation::Inner;
                    })
                }),
                told(
                    "no faces run along the neutral face's loops it propagates to",
                    "Pick faces to draft, or choose other loops under Propagation.",
                ),
            ),
            (
                Box::new(move |e| draft(e, &|d| d.faces = vec![FaceId::side(e, 9)])),
                told("the faces it drafts are no longer on the part", "Edit it and pick the faces to draft again."),
            ),
            (
                // The top face is parallel to the neutral plane: the kernel's own words.
                Box::new(move |e| draft(e, &|d| d.faces = vec![FaceId { feature: e, index: FaceId::END_CAP }])),
                told(
                    "a face parallel to the neutral plane cannot be drafted",
                    "Try a smaller angle, or draft other faces: flat faces square to the neutral plane, \
                     and round faces whose axis runs along the pull.",
                ),
            ),
        ];
        for (make, want) in cases {
            let mut doc = Document::new();
            let e = boxed(&mut doc, (0.0, 0.0), (20.0, 20.0), PlaneRef::Front, 10.0);
            let id = doc.add_feature(make(e));
            assert_eq!(crate::refgeom::failure_of(&mut doc, id), want);
            assert!((exact_volume(&doc.rebuild()) - 4000.0).abs() < 1e-6);
        }

        // On a part an elliptical dome has made faceted, the dome is named.
        let mut doc = Document::new();
        let e = cylinder(&mut doc);
        let top = FaceId { feature: e, index: FaceId::END_CAP };
        doc.add_feature(FeatureKind::Dome(DomeFeature {
            faces: vec![top],
            height: Dim::Value(5.0),
            reverse: false,
            elliptical: true,
        }));
        let id = doc.add_feature(FeatureKind::Draft(DraftFeature {
            neutral: PlaneRef::Front,
            faces: vec![FaceId::side(e, 0)],
            angle: Dim::Value(5.0),
            reverse: false,
            propagation: Propagation::None,
        }));
        assert_eq!(
            crate::refgeom::failure_of(&mut doc, id),
            told(
                "drafts need the exact kernel, and the part is faceted from Dome1 on",
                "Fix or suppress Dome1, so the part stays exact up to here."
            )
        );
    }

    #[test]
    fn dome_failures_say_what_to_change() {
        let dome = |faces: Vec<FaceId>, h: Dim| {
            FeatureKind::Dome(DomeFeature { faces, height: h, reverse: false, elliptical: false })
        };
        let cases: Vec<(Make, (String, String))> = vec![
            (
                Box::new(move |e| {
                    dome(vec![FaceId { feature: e, index: FaceId::END_CAP }], Dim::Value(-1.0))
                }),
                told(
                    "the height (-1 mm) must not be negative",
                    "Give it a height of 0 or more; 0 makes a half sphere, and Reverse cuts the dome in.",
                ),
            ),
            (
                Box::new(move |_| dome(vec![], Dim::Value(5.0))),
                told("it has no face to dome", "Edit it and pick a flat round face."),
            ),
            (
                // The cylinder's round side.
                Box::new(move |e| dome(vec![FaceId::side(e, 0)], Dim::Value(5.0))),
                told(
                    "the face to dome is not flat, or is no longer on the part",
                    "Edit it and pick a flat round face.",
                ),
            ),
        ];
        for (make, want) in cases {
            let mut doc = Document::new();
            let e = cylinder(&mut doc);
            let id = doc.add_feature(make(e));
            assert_eq!(crate::refgeom::failure_of(&mut doc, id), want);
            let st = doc.rebuild();
            assert!(st.exact.is_some(), "a failed dome leaves the part exact");
            let v = exact_volume(&st);
            // The mesh of the round side is within 2e-5 of it; any dome would add over 200 mm³.
            assert!((v - 1000.0 * std::f64::consts::PI).abs() < 2e-5 * v, "{v}");
        }
    }
}
