//! Multibody parts: body names, and the features that work on whole bodies (Combine,
//! Move/Copy, Delete/Keep, Split, Scale). The behaviour is specified in
//! `docs/re/multibody.md`. Bodies live on the exact kernel; these features need it.

use crate::document::*;
use crate::exact;
use crate::rebuild::BuildState;
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

const EXACT_ONLY: &str =
    "body features need the exact kernel, and the part is faceted from an earlier feature on";

fn d(v: Vec3) -> DVec3 {
    DVec3::new(v.x, v.y, v.z)
}

impl BuildState {
    /// Indices of the named bodies (all of them for an empty list).
    fn pick(&self, refs: &[BodyRef]) -> Result<Vec<usize>, String> {
        if refs.is_empty() {
            return Ok((0..self.body_names.len()).collect());
        }
        refs.iter()
            .map(|r| {
                self.body_names
                    .iter()
                    .position(|n| n == r)
                    .ok_or_else(|| format!("body {}[{}] no longer exists", r.feature, r.index))
            })
            .collect()
    }

    /// Replaces the exact bodies with `out`, as feature `id`.
    fn set_bodies(&mut self, id: u32, out: Vec<brep::Solid>) -> Result<(), String> {
        if self.exact_step(id, |_| Ok(out)) {
            Ok(())
        } else {
            Err(self.faceted_from.as_ref().map(|f| f.1.clone()).unwrap_or_else(|| EXACT_ONLY.into()))
        }
    }
}

impl Document {
    pub(crate) fn rebuild_combine(
        &self,
        id: u32,
        c: &CombineFeature,
        st: &mut BuildState,
    ) -> Result<(), String> {
        let bodies = st.exact.clone().ok_or(EXACT_ONLY)?;
        let mut picked = st.pick(&c.bodies)?;
        if let (CombineOp::Subtract, Some(m)) = (c.op, &c.main) {
            let mi = st.pick(std::slice::from_ref(m))?[0];
            picked.retain(|&i| i != mi);
            picked.insert(0, mi);
        }
        if picked.len() < 2 {
            return Err("choose at least two bodies to combine".into());
        }
        let e = |k: brep::KernelError| k.to_string();
        let mut acc = vec![bodies[picked[0]].clone()];
        for &i in &picked[1..] {
            let op = match c.op {
                CombineOp::Add => BoolOp::Union,
                CombineOp::Subtract => BoolOp::Subtract,
                CombineOp::Common => BoolOp::Intersect,
            };
            let mut next = Vec::new();
            for a in &acc {
                next.extend(boolean(a, &bodies[i], op).map_err(e)?);
            }
            acc = next;
        }
        match c.op {
            CombineOp::Add if acc.len() != 1 => return Err("the bodies do not touch".into()),
            CombineOp::Common if acc.is_empty() => return Err("the bodies have nothing in common".into()),
            CombineOp::Subtract if acc.is_empty() => return Err("nothing of the main body is left".into()),
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
    ) -> Result<(), String> {
        let num = |x: &Dim, what: &str| x.eval(&st.variables).map_err(|e| format!("{what}: {e}"));
        let t =
            DVec3::new(num(&m.translate[0], "X")?, num(&m.translate[1], "Y")?, num(&m.translate[2], "Z")?);
        let r = [num(&m.rotate[0], "X angle")?, num(&m.rotate[1], "Y angle")?, num(&m.rotate[2], "Z angle")?];
        let bodies = st.exact.clone().ok_or(EXACT_ONLY)?;
        let picked = st.pick(&m.bodies)?;
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
    ) -> Result<(), String> {
        let bodies = st.exact.clone().ok_or(EXACT_ONLY)?;
        if dl.bodies.is_empty() {
            return Err("choose the bodies to delete or keep".into());
        }
        let picked = st.pick(&dl.bodies)?;
        let out = bodies
            .into_iter()
            .enumerate()
            .filter(|(i, _)| picked.contains(i) == dl.keep)
            .map(|(_, b)| b)
            .collect();
        st.set_bodies(id, out)
    }

    pub(crate) fn rebuild_split(
        &self,
        id: u32,
        sp: &SplitFeature,
        st: &mut BuildState,
    ) -> Result<(), String> {
        let bodies = st.exact.clone().ok_or(EXACT_ONLY)?;
        if sp.planes.is_empty() {
            return Err("choose a plane to split along".into());
        }
        let picked = st.pick(&sp.bodies)?;
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
                    next.extend(boolean(b, half, BoolOp::Intersect).map_err(|e| e.to_string())?);
                }
            }
            pieces = next;
        }
        out.extend(pieces);
        st.set_bodies(id, out)
    }

    pub(crate) fn rebuild_scale(
        &self,
        id: u32,
        sc: &ScaleFeature,
        st: &mut BuildState,
    ) -> Result<(), String> {
        let num = |x: &Dim, what: &str| x.eval(&st.variables).map_err(|e| format!("{what}: {e}"));
        let k = [
            num(&sc.factors[0], "X factor")?,
            num(&sc.factors[1], "Y factor")?,
            num(&sc.factors[2], "Z factor")?,
        ];
        let bodies = st.exact.clone().ok_or(EXACT_ONLY)?;
        let picked = st.pick(&sc.bodies)?;
        let (center, axes) = match sc.about {
            ScaleAbout::Origin => (DVec3::ZERO, brep::Frame::world()),
            ScaleAbout::CoordSys(cs) => {
                let f = st.frames.get(&cs).ok_or("the coordinate system failed or is missing")?;
                (d(f.origin), exact::frame(f))
            }
            ScaleAbout::Centroid => {
                let params = brep::tess::TessParams::default();
                let (mut v, mut c) = (0.0, DVec3::ZERO);
                for &i in &picked {
                    let m = brep::mass::properties(
                        &brep::tess::tessellate(&bodies[i], &params).map_err(|e| e.to_string())?,
                    );
                    v += m.volume;
                    c += m.centroid * m.volume;
                }
                (if v > 0.0 { c / v } else { DVec3::ZERO }, brep::Frame::world())
            }
        };
        let mut out = bodies.clone();
        for &i in &picked {
            out[i] = brep::scale::scaled(&bodies[i], center, &axes, k).map_err(|e| e.to_string())?;
        }
        st.set_bodies(id, out)
    }
}

#[cfg(test)]
mod tests {
    //! Statements from `docs/re/multibody.md`, numbered as there.
    use super::*;
    use anvil_kernel::v2;

    /// A new part with a box per rectangle, each its own body (merge result off).
    fn boxes(rects: &[((f64, f64), (f64, f64), f64)]) -> (Document, Vec<u32>) {
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

    const A: ((f64, f64), (f64, f64), f64) = ((0.0, 0.0), (10.0, 10.0), 10.0);
    const B: ((f64, f64), (f64, f64), f64) = ((5.0, 0.0), (15.0, 10.0), 10.0);
    const D: ((f64, f64), (f64, f64), f64) = ((20.0, 0.0), (30.0, 10.0), 10.0);

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
}
