//! Multibody parts: body names, and the features that work on whole bodies (Combine,
//! Move/Copy, Delete/Keep, Split, Scale). The behaviour is specified in
//! `docs/re/multibody.md`. Bodies live on the exact kernel; these features need it.

use crate::document::*;
use crate::exact;
use crate::failure::Cause;
use crate::rebuild::BuildState;
use crate::refgeom::{num_text, value};
use anvil_brep as brep;
use anvil_brep::boolean::{boolean, BoolOp};
use anvil_kernel::Vec3;
use glam::DVec3;
use serde::{Deserialize, Serialize};

/// A body named by the feature that last changed it and its place (1-based) among the
/// bodies that feature left, ordered by the lowest corner of their bounding boxes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BodyRef {
    pub feature: u32,
    pub index: u32,
}

/// Names the bodies after a feature: a body the feature left as it was keeps its name;
/// the others are named after the feature.
pub(crate) fn rename(
    old_names: &[BodyRef],
    old: &[brep::Solid],
    new: &[brep::Solid],
    feature: u32,
) -> Vec<BodyRef> {
    let key = |s: &brep::Solid| {
        let lo = s.bounds().map(|b| b.0).unwrap_or(DVec3::ZERO);
        let r = |v: f64| (v * 1e6).round() as i64;
        (r(lo.x), r(lo.y), r(lo.z))
    };
    let mut fresh: Vec<usize> = Vec::new();
    let mut names: Vec<Option<BodyRef>> = new
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let kept = old.iter().position(|o| o == b).and_then(|j| old_names.get(j).copied());
            if kept.is_none() {
                fresh.push(i);
            }
            kept
        })
        .collect();
    fresh.sort_by_key(|&i| key(&new[i]));
    for (n, i) in fresh.into_iter().enumerate() {
        names[i] = Some(BodyRef { feature, index: n as u32 + 1 });
    }
    names.into_iter().map(|n| n.unwrap_or(BodyRef { feature, index: 0 })).collect()
}

/// How Combine joins bodies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CombineOp {
    #[default]
    Add,
    /// The main body less the others.
    Subtract,
    Common,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CombineFeature {
    pub op: CombineOp,
    /// For Subtract: the body kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<BodyRef>,
    pub bodies: Vec<BodyRef>,
}

/// Moves bodies, or copies of them: turned about the centre of their bounding box (or
/// `origin`) by X, then Y, then Z angles (degrees, about the part's axes), then moved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoveCopyFeature {
    /// The bodies to move; every body when empty.
    #[serde(default)]
    pub bodies: Vec<BodyRef>,
    #[serde(default)]
    pub copies: u32,
    pub translate: [Dim; 3],
    pub rotate: [Dim; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Vec3>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeleteBodyFeature {
    pub bodies: Vec<BodyRef>,
    /// Keep the listed bodies and delete the rest.
    #[serde(default)]
    pub keep: bool,
}

/// Cuts bodies in two along planes; every piece becomes a body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SplitFeature {
    pub planes: Vec<PlaneRef>,
    /// The bodies to split; every body when empty.
    #[serde(default)]
    pub bodies: Vec<BodyRef>,
}

/// Where a scale is centred.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum ScaleAbout {
    #[default]
    Centroid,
    Origin,
    /// A coordinate system feature: its origin and axes.
    CoordSys(u32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScaleFeature {
    #[serde(default)]
    pub bodies: Vec<BodyRef>,
    #[serde(default)]
    pub about: ScaleAbout,
    pub factors: [Dim; 3],
}

fn d(v: Vec3) -> DVec3 {
    DVec3::new(v.x, v.y, v.z)
}

impl BuildState {
    /// Replaces the exact bodies with `out`, as feature `id`.
    fn set_bodies(&mut self, id: u32, out: Vec<brep::Solid>) -> Result<(), Cause> {
        if self.exact_step(id, |_| Ok(out)) {
            Ok(())
        } else {
            Err(self
                .faceted_from
                .as_ref()
                .map(|f| Cause::from(f.1.clone()))
                .unwrap_or_else(|| "the exact kernel could not build the result".into()))
        }
    }
}

impl Document {
    /// Why a feature that only the exact kernel builds (`what`, as in "body features")
    /// cannot be built on this part, naming the feature where the part became faceted.
    pub(crate) fn exact_only(&self, what: &str, st: &BuildState) -> Cause {
        match st.faceted_from.as_ref().and_then(|(f, _)| self.feature(*f)) {
            Some(f) => Cause::told(
                format!("{what} need the exact kernel, and the part is faceted from {} on", f.name),
                format!("Fix or suppress {}, so the part stays exact up to here.", f.name),
            ),
            None => Cause::told(
                format!("{what} need the exact kernel, and the part is faceted"),
                "Build the part from features the exact kernel supports, such as sketches, extrudes and revolves.",
            ),
        }
    }

    /// The exact bodies, for a feature that only works on them.
    fn exact_bodies(&self, st: &BuildState) -> Result<Vec<brep::Solid>, Cause> {
        st.exact.clone().ok_or_else(|| self.exact_only("body features", st))
    }

    /// Indices of the named bodies (all of them for an empty list).
    fn pick(&self, st: &BuildState, refs: &[BodyRef]) -> Result<Vec<usize>, Cause> {
        if refs.is_empty() {
            return Ok((0..st.body_names.len()).collect());
        }
        refs.iter()
            .map(|r| {
                st.body_names.iter().position(|n| n == r).ok_or_else(|| {
                    let from =
                        self.feature(r.feature).map(|f| f.name.as_str()).unwrap_or("a deleted feature");
                    Cause::told(
                        format!("a body it uses, made by {from}, no longer exists"),
                        "Edit it and pick the bodies again.",
                    )
                })
            })
            .collect()
    }

    pub(crate) fn rebuild_combine(
        &self,
        id: u32,
        c: &CombineFeature,
        st: &mut BuildState,
    ) -> Result<(), Cause> {
        let bodies = self.exact_bodies(st)?;
        let mut picked = self.pick(st, &c.bodies)?;
        if let (CombineOp::Subtract, Some(m)) = (c.op, &c.main) {
            let mi = self.pick(st, std::slice::from_ref(m))?[0];
            picked.retain(|&i| i != mi);
            picked.insert(0, mi);
        }
        if picked.len() < 2 {
            return Err(Cause::told(
                format!("it needs at least two bodies to combine, and has {}", picked.len()),
                "Edit it and pick two or more bodies.",
            ));
        }
        let mut acc = vec![bodies[picked[0]].clone()];
        for &i in &picked[1..] {
            let op = match c.op {
                CombineOp::Add => BoolOp::Union,
                CombineOp::Subtract => BoolOp::Subtract,
                CombineOp::Common => BoolOp::Intersect,
            };
            let mut next = Vec::new();
            for a in &acc {
                next.extend(boolean(a, &bodies[i], op)?);
            }
            acc = next;
        }
        match c.op {
            CombineOp::Add if acc.len() != 1 => {
                return Err(Cause::told(
                    "the bodies do not touch",
                    "Move one body so they overlap, or pick other bodies.",
                ))
            }
            CombineOp::Common if acc.is_empty() => {
                return Err(Cause::told(
                    "the bodies have nothing in common",
                    "Pick bodies that overlap, or use Add to join them.",
                ))
            }
            CombineOp::Subtract if acc.is_empty() => {
                return Err(Cause::told(
                    "nothing of the main body is left",
                    "Pick another main body, or fewer bodies to subtract from it.",
                ))
            }
            _ => {}
        }
        for s in &mut acc {
            for f in &mut s.faces {
                if f.owner == 0 {
                    f.owner = id;
                }
            }
        }
        let mut out: Vec<brep::Solid> =
            bodies.iter().enumerate().filter(|(i, _)| !picked.contains(i)).map(|(_, b)| b.clone()).collect();
        out.extend(acc);
        st.set_bodies(id, out)
    }

    pub(crate) fn rebuild_move_copy(
        &self,
        id: u32,
        m: &MoveCopyFeature,
        st: &mut BuildState,
    ) -> Result<(), Cause> {
        let num = |x: &Dim, what: &str| value(x, &st.variables, what);
        let t = DVec3::new(
            num(&m.translate[0], "X distance")?,
            num(&m.translate[1], "Y distance")?,
            num(&m.translate[2], "Z distance")?,
        );
        let r = [num(&m.rotate[0], "X angle")?, num(&m.rotate[1], "Y angle")?, num(&m.rotate[2], "Z angle")?];
        let bodies = self.exact_bodies(st)?;
        let picked = self.pick(st, &m.bodies)?;
        let center = match m.origin {
            Some(o) => d(o),
            None => {
                let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
                for &i in &picked {
                    if let Some((a, b)) = bodies[i].bounds() {
                        lo = lo.min(a);
                        hi = hi.max(b);
                    }
                }
                (lo + hi) * 0.5
            }
        };
        // p' = c + Rz Ry Rx (p - c) + k t, applied k times for copy k.
        let motion = |k: u32| -> brep::Frame {
            let mut x = DVec3::X;
            let mut y = DVec3::Y;
            let mut z = DVec3::Z;
            for _ in 0..k {
                for (axis, a) in [(DVec3::X, r[0]), (DVec3::Y, r[1]), (DVec3::Z, r[2])] {
                    let turn = |v: DVec3| brep::sweep::rotate(v, DVec3::ZERO, axis, a.to_radians());
                    (x, y, z) = (turn(x), turn(y), turn(z));
                }
            }
            let origin = center - (x * center.x + y * center.y + z * center.z) + t * k as f64;
            brep::Frame { origin, x, y, z }
        };
        let mut out = bodies.clone();
        if m.copies == 0 {
            for &i in &picked {
                out[i] = bodies[i].transformed(&motion(1));
            }
        } else {
            for k in 1..=m.copies {
                for &i in &picked {
                    out.push(bodies[i].transformed(&motion(k)));
                }
            }
        }
        st.set_bodies(id, out)
    }

    pub(crate) fn rebuild_delete_body(
        &self,
        id: u32,
        dl: &DeleteBodyFeature,
        st: &mut BuildState,
    ) -> Result<(), Cause> {
        let bodies = self.exact_bodies(st)?;
        if dl.bodies.is_empty() {
            let what = if dl.keep { "keep" } else { "delete" };
            return Err(Cause::told(
                format!("it has no bodies to {what}"),
                format!("Edit it and pick the bodies to {what}."),
            ));
        }
        let picked = self.pick(st, &dl.bodies)?;
        let out = bodies
            .into_iter()
            .enumerate()
            .filter(|(i, _)| picked.contains(i) == dl.keep)
            .map(|(_, b)| b)
            .collect();
        st.set_bodies(id, out)
    }

    pub(crate) fn rebuild_split(&self, id: u32, sp: &SplitFeature, st: &mut BuildState) -> Result<(), Cause> {
        let bodies = self.exact_bodies(st)?;
        if sp.planes.is_empty() {
            return Err(Cause::told(
                "it has no plane to split along",
                "Edit it and pick a plane or a flat face.",
            ));
        }
        let picked = self.pick(st, &sp.bodies)?;
        let reach = st.body.bounds().diagonal() + st.body.bounds().center().length() + 10.0;
        let mut out: Vec<brep::Solid> =
            bodies.iter().enumerate().filter(|(i, _)| !picked.contains(i)).map(|(_, b)| b.clone()).collect();
        let mut pieces: Vec<brep::Solid> = picked.iter().map(|&i| bodies[i].clone()).collect();
        for p in &sp.planes {
            let f = self.resolve_plane(p, st)?;
            // The two half-spaces as big boxes either side of the plane.
            let sq = anvil_kernel::Profile::new(
                anvil_kernel::Loop2::rectangle(
                    anvil_kernel::v2(-2.0 * reach, -2.0 * reach),
                    anvil_kernel::v2(2.0 * reach, 2.0 * reach),
                    0,
                ),
                vec![],
            );
            let frame = anvil_kernel::Frame {
                origin: f.origin - f.x * f.x.dot(f.origin) - f.y * f.y.dot(f.origin),
                ..f
            };
            let above = exact::extrude(std::slice::from_ref(&sq), &frame, 0.0, 4.0 * reach, id)?;
            let below = exact::extrude(std::slice::from_ref(&sq), &frame, -4.0 * reach, 0.0, id)?;
            let mut next = Vec::new();
            for b in &pieces {
                for half in above.iter().chain(&below) {
                    next.extend(boolean(b, half, BoolOp::Intersect)?);
                }
            }
            pieces = next;
        }
        out.extend(pieces);
        st.set_bodies(id, out)
    }

    pub(crate) fn rebuild_scale(&self, id: u32, sc: &ScaleFeature, st: &mut BuildState) -> Result<(), Cause> {
        let num = |x: &Dim, what: &str| value(x, &st.variables, what);
        let k = [
            num(&sc.factors[0], "X scale factor")?,
            num(&sc.factors[1], "Y scale factor")?,
            num(&sc.factors[2], "Z scale factor")?,
        ];
        let bodies = self.exact_bodies(st)?;
        let picked = self.pick(st, &sc.bodies)?;
        // The kernel refuses these too; saying which factor is wrong helps more.
        if !picked.is_empty() {
            if let Some((axis, f)) =
                ["X", "Y", "Z"].into_iter().zip(k).find(|(_, f)| !f.is_finite() || *f <= 0.0)
            {
                return Err(Cause::told(
                    format!("the {axis} scale factor ({}) must be greater than zero", num_text(f)),
                    "Give each factor a value above 0; a factor of 1 leaves that direction as it is.",
                ));
            }
        }
        let (center, axes) = match sc.about {
            ScaleAbout::Origin => (DVec3::ZERO, brep::Frame::world()),
            ScaleAbout::CoordSys(cs) => {
                let f = st.frames.get(&cs).ok_or_else(|| {
                    Cause::told(
                        "the coordinate system it scales about failed or is missing",
                        "Fix or unsuppress the coordinate system, or scale about the centroid or the origin.",
                    )
                })?;
                (d(f.origin), exact::frame(f))
            }
            ScaleAbout::Centroid => {
                let params = brep::tess::TessParams::default();
                let (mut v, mut c) = (0.0, DVec3::ZERO);
                for &i in &picked {
                    let m = brep::mass::properties(&brep::tess::tessellate(&bodies[i], &params)?);
                    v += m.volume;
                    c += m.centroid * m.volume;
                }
                (if v > 0.0 { c / v } else { DVec3::ZERO }, brep::Frame::world())
            }
        };
        let mut out = bodies.clone();
        for &i in &picked {
            out[i] = brep::scale::scaled(&bodies[i], center, &axes, k).map_err(|e| match e {
                // The only thing the scale refuses as unsupported: round faces scaled unevenly.
                brep::KernelError::Unsupported(_) => Cause::told(
                    "scaling by different factors would make its round faces elliptical, which the modeler cannot build yet",
                    "Use the same factor in X, Y and Z for a body with round faces.",
                ),
                e => e.into(),
            })?;
        }
        st.set_bodies(id, out)
    }
}

#[cfg(test)]
mod tests {
    //! Statements from `docs/re/multibody.md`, numbered as there.
    use super::*;
    use anvil_kernel::v2;

    /// A rectangle's two corners and the box's height.
    type Boxed = ((f64, f64), (f64, f64), f64);

    /// A new part with a box per rectangle, each its own body (merge result off).
    fn boxes(rects: &[Boxed]) -> (Document, Vec<u32>) {
        let mut doc = Document::new();
        let mut ids = Vec::new();
        for &(a, b, h) in rects {
            let mut sf = SketchFeature::new(PlaneRef::Front);
            sf.sketch.add_rectangle(v2(a.0, a.1), v2(b.0, b.1));
            let s = doc.add_feature(FeatureKind::Sketch(sf));
            ids.push(doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
                sketch: s,
                depth: Dim::Value(h),
                merge: false,
                ..Default::default()
            })));
        }
        (doc, ids)
    }

    fn body(f: u32) -> BodyRef {
        BodyRef { feature: f, index: 1 }
    }

    fn volumes(st: &BuildState) -> Vec<f64> {
        let p = brep::tess::TessParams::default();
        let mut v: Vec<f64> = st
            .exact
            .iter()
            .flatten()
            .map(|b| {
                (brep::mass::properties(&brep::tess::tessellate(b, &p).unwrap()).volume * 1e6).round() / 1e6
            })
            .collect();
        v.sort_by(f64::total_cmp);
        v
    }

    fn built(doc: &mut Document) -> BuildState {
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        st
    }

    const A: Boxed = ((0.0, 0.0), (10.0, 10.0), 10.0);
    const B: Boxed = ((5.0, 0.0), (15.0, 10.0), 10.0);
    const D: Boxed = ((20.0, 0.0), (30.0, 10.0), 10.0);

    #[test]
    fn s01_merge_result_off_makes_bodies() {
        let (mut doc, _) = boxes(&[A, B]);
        let st = built(&mut doc);
        assert_eq!(volumes(&st), vec![1000.0, 1000.0]);
        assert!((st.body.mass_properties().volume - 2000.0).abs() < 1e-6);
    }

    #[test]
    fn s05_s06_s07_s09_s10_combine() {
        for (op, main, want) in [
            (CombineOp::Common, None, Some(vec![500.0])),
            (CombineOp::Add, None, Some(vec![1500.0])),
            (CombineOp::Subtract, Some(0), Some(vec![500.0])),
        ] {
            let (mut doc, ids) = boxes(&[A, B]);
            doc.add_feature(FeatureKind::Combine(CombineFeature {
                op,
                main: main.map(|i: usize| body(ids[i])),
                bodies: ids.iter().map(|&i| body(i)).collect(),
            }));
            assert_eq!(Some(volumes(&built(&mut doc))), want);
        }
        let (mut doc, ids) = boxes(&[A, D]);
        let c = doc.add_feature(FeatureKind::Combine(CombineFeature {
            op: CombineOp::Common,
            main: None,
            bodies: ids.iter().map(|&i| body(i)).collect(),
        }));
        assert!(doc.rebuild().errors.contains_key(&c));
    }

    #[test]
    fn s11_subtract_into_two_named_pieces() {
        let (mut doc, ids) = boxes(&[((0.0, 0.0), (30.0, 10.0), 10.0), ((14.0, -1.0), (16.0, 11.0), 10.0)]);
        let c = doc.add_feature(FeatureKind::Combine(CombineFeature {
            op: CombineOp::Subtract,
            main: Some(body(ids[0])),
            bodies: vec![body(ids[1])],
        }));
        let st = built(&mut doc);
        assert_eq!(volumes(&st), vec![1400.0, 1400.0]);
        assert_eq!(st.body_names, vec![BodyRef { feature: c, index: 1 }, BodyRef { feature: c, index: 2 }]);
        // [1] is the piece nearer the origin.
        let lo = st.exact.as_ref().unwrap()[0].bounds().unwrap().0;
        assert!(lo.x.abs() < 1e-9);
    }

    #[test]
    fn s14_s17_s18_move_and_copy() {
        let (mut doc, ids) = boxes(&[A]);
        let v = |x: f64| Dim::Value(x);
        let m = doc.add_feature(FeatureKind::MoveCopy(MoveCopyFeature {
            bodies: vec![body(ids[0])],
            copies: 0,
            translate: [v(20.0), v(0.0), v(0.0)],
            rotate: [v(0.0), v(0.0), v(0.0)],
            origin: None,
        }));
        let st = built(&mut doc);
        assert!((st.body.mass_properties().centroid.x - 25.0).abs() < 1e-9);
        if let Some(FeatureKind::MoveCopy(x)) = doc.feature_mut(m).map(|f| &mut f.kind) {
            x.copies = 2;
        }
        let st = built(&mut doc);
        assert_eq!(volumes(&st).len(), 3);
        assert!((st.body.mass_properties().centroid.x - 25.0).abs() < 1e-9);

        let (mut doc, ids) = boxes(&[((0.0, 0.0), (10.0, 20.0), 30.0)]);
        doc.add_feature(FeatureKind::MoveCopy(MoveCopyFeature {
            bodies: vec![body(ids[0])],
            copies: 0,
            translate: [v(0.0), v(0.0), v(0.0)],
            rotate: [v(0.0), v(0.0), v(90.0)],
            origin: None,
        }));
        let b = built(&mut doc).body.bounds();
        assert!(
            (b.min.x + 5.0).abs() < 1e-9 && (b.max.x - 15.0).abs() < 1e-9 && (b.min.y - 5.0).abs() < 1e-9
        );
    }

    #[test]
    fn s21_delete_and_keep() {
        for (keep, want) in [(false, 1000.0), (true, 1000.0)] {
            let (mut doc, ids) = boxes(&[A, D]);
            doc.add_feature(FeatureKind::DeleteBody(DeleteBodyFeature { bodies: vec![body(ids[1])], keep }));
            let st = built(&mut doc);
            assert_eq!(volumes(&st), vec![want]);
            let cx = st.body.mass_properties().centroid.x;
            assert!((cx - if keep { 25.0 } else { 5.0 }).abs() < 1e-9);
        }
    }

    #[test]
    fn s22_split_with_planes() {
        let (mut doc, _) = boxes(&[A]);
        let p3 = doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base: PlaneRef::Right,
            offset: Dim::Value(3.0),
            ..Default::default()
        }));
        let p7 = doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base: PlaneRef::Right,
            offset: Dim::Value(7.0),
            ..Default::default()
        }));
        let s = doc.add_feature(FeatureKind::Split(SplitFeature {
            planes: vec![PlaneRef::Feature(p3)],
            bodies: vec![],
        }));
        assert_eq!(volumes(&built(&mut doc)), vec![300.0, 700.0]);
        if let Some(FeatureKind::Split(x)) = doc.feature_mut(s).map(|f| &mut f.kind) {
            x.planes.push(PlaneRef::Feature(p7));
        }
        assert_eq!(volumes(&built(&mut doc)), vec![300.0, 300.0, 400.0]);
    }

    #[test]
    fn s26_s27_s28_scale() {
        let v = |x: f64| Dim::Value(x);
        for (about, k, want, lo) in [
            (ScaleAbout::Origin, [3.0, 3.0, 3.0], 27000.0, 0.0),
            (ScaleAbout::Origin, [3.0, 1.0, 1.0], 3000.0, 0.0),
            (ScaleAbout::Centroid, [2.0, 2.0, 2.0], 8000.0, -5.0),
        ] {
            let (mut doc, _) = boxes(&[A]);
            doc.add_feature(FeatureKind::Scale(ScaleFeature { bodies: vec![], about, factors: k.map(v) }));
            let st = built(&mut doc);
            assert_eq!(volumes(&st), vec![want]);
            assert!((st.body.bounds().min.x - lo).abs() < 1e-9);
        }
    }

    #[test]
    fn feature_names() {
        let (mut doc, ids) = boxes(&[A, B]);
        let c = doc.add_feature(FeatureKind::Combine(CombineFeature {
            op: CombineOp::Add,
            main: None,
            bodies: ids.iter().map(|&i| body(i)).collect(),
        }));
        assert_eq!(doc.feature(c).map(|f| f.name.as_str()), Some("Combine1"));
    }

    fn told(c: &str, s: &str) -> (String, String) {
        (c.to_string(), s.to_string())
    }

    /// Each way a body feature can fail says what to change; the bodies stay as they were.
    #[test]
    fn body_feature_failures_say_what_to_change() {
        let v = |x: f64| Dim::Value(x);
        let combine = |op: CombineOp, main: Option<BodyRef>, bodies: Vec<BodyRef>| {
            FeatureKind::Combine(CombineFeature { op, main, bodies })
        };
        // Inside the 12 x 12 x 12 box: A is subtracted to nothing.
        const BIG: Boxed = ((-1.0, -1.0), (11.0, 11.0), 12.0);
        type Make = Box<dyn Fn(&[u32]) -> FeatureKind>;
        let cases: Vec<(&[Boxed], Make, (String, String))> = vec![
            (
                &[A, D],
                Box::new(move |ids| combine(CombineOp::Add, None, ids.iter().map(|&i| body(i)).collect())),
                told("the bodies do not touch", "Move one body so they overlap, or pick other bodies."),
            ),
            (
                &[A, D],
                Box::new(move |ids| combine(CombineOp::Common, None, ids.iter().map(|&i| body(i)).collect())),
                told(
                    "the bodies have nothing in common",
                    "Pick bodies that overlap, or use Add to join them.",
                ),
            ),
            (
                &[A, BIG],
                Box::new(move |ids| {
                    combine(CombineOp::Subtract, Some(body(ids[0])), ids.iter().map(|&i| body(i)).collect())
                }),
                told(
                    "nothing of the main body is left",
                    "Pick another main body, or fewer bodies to subtract from it.",
                ),
            ),
            (
                &[A, B],
                Box::new(move |ids| combine(CombineOp::Add, None, vec![body(ids[0])])),
                told(
                    "it needs at least two bodies to combine, and has 1",
                    "Edit it and pick two or more bodies.",
                ),
            ),
            (
                &[A, B],
                Box::new(move |ids| {
                    combine(CombineOp::Add, None, vec![body(ids[0]), BodyRef { feature: ids[1], index: 5 }])
                }),
                told(
                    "a body it uses, made by Boss-Extrude2, no longer exists",
                    "Edit it and pick the bodies again.",
                ),
            ),
            (
                &[A, D],
                Box::new(|_| FeatureKind::DeleteBody(DeleteBodyFeature { bodies: vec![], keep: false })),
                told("it has no bodies to delete", "Edit it and pick the bodies to delete."),
            ),
            (
                &[A, D],
                Box::new(|_| FeatureKind::DeleteBody(DeleteBodyFeature { bodies: vec![], keep: true })),
                told("it has no bodies to keep", "Edit it and pick the bodies to keep."),
            ),
            (
                &[A, D],
                Box::new(|_| FeatureKind::Split(SplitFeature { planes: vec![], bodies: vec![] })),
                told("it has no plane to split along", "Edit it and pick a plane or a flat face."),
            ),
            (
                &[A, D],
                Box::new(move |_| {
                    FeatureKind::Scale(ScaleFeature {
                        bodies: vec![],
                        about: ScaleAbout::Origin,
                        factors: [v(2.0), v(0.0), v(2.0)],
                    })
                }),
                told(
                    "the Y scale factor (0) must be greater than zero",
                    "Give each factor a value above 0; a factor of 1 leaves that direction as it is.",
                ),
            ),
            (
                &[A, D],
                Box::new(move |_| {
                    FeatureKind::Scale(ScaleFeature {
                        bodies: vec![],
                        about: ScaleAbout::CoordSys(999),
                        factors: [v(2.0), v(2.0), v(2.0)],
                    })
                }),
                told(
                    "the coordinate system it scales about failed or is missing",
                    "Fix or unsuppress the coordinate system, or scale about the centroid or the origin.",
                ),
            ),
            (
                &[A, D],
                Box::new(move |_| {
                    FeatureKind::MoveCopy(MoveCopyFeature {
                        bodies: vec![],
                        copies: 0,
                        translate: [Dim::Expr("q".into()), v(0.0), v(0.0)],
                        rotate: [v(0.0), v(0.0), v(0.0)],
                        origin: None,
                    })
                }),
                told(
                    "the X distance cannot be worked out: unknown variable 'q'",
                    "Edit the X distance and fix its equation, or define the variables it uses in Equations.",
                ),
            ),
        ];
        for (rects, make, want) in cases {
            let (mut doc, ids) = boxes(rects);
            let before = volumes(&built(&mut doc));
            let id = doc.add_feature(make(&ids));
            assert_eq!(crate::refgeom::failure_of(&mut doc, id), want);
            assert_eq!(volumes(&doc.rebuild()), before);
        }
    }

    #[test]
    fn scaling_round_faces_unevenly_says_use_one_factor() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        sf.sketch.add_circle(c, 10.0);
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature { sketch: s, ..Default::default() }));
        let id = doc.add_feature(FeatureKind::Scale(ScaleFeature {
            bodies: vec![],
            about: ScaleAbout::Origin,
            factors: [Dim::Value(2.0), Dim::Value(1.0), Dim::Value(1.0)],
        }));
        assert_eq!(
            crate::refgeom::failure_of(&mut doc, id),
            told(
                "scaling by different factors would make its round faces elliptical, which the modeler cannot build yet",
                "Use the same factor in X, Y and Z for a body with round faces."
            )
        );
    }

    #[test]
    fn body_features_on_a_faceted_part_name_where_it_became_faceted() {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let c = sf.sketch.add_point(v2(0.0, 0.0));
        sf.sketch.add_circle(c, 10.0);
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        let e = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature { sketch: s, ..Default::default() }));
        doc.add_feature(FeatureKind::Dome(crate::modify::DomeFeature {
            faces: vec![anvil_kernel::FaceId { feature: e, index: anvil_kernel::FaceId::END_CAP }],
            height: Dim::Value(5.0),
            reverse: false,
            elliptical: true,
        }));
        let id = doc.add_feature(FeatureKind::DeleteBody(DeleteBodyFeature { bodies: vec![], keep: false }));
        assert_eq!(
            crate::refgeom::failure_of(&mut doc, id),
            told(
                "body features need the exact kernel, and the part is faceted from Dome1 on",
                "Fix or suppress Dome1, so the part stays exact up to here."
            )
        );
    }
}
