//! Lofted bosses and cuts between two profiles: each a closed loop or a single point,
//! joined by ruled faces. The behaviour is specified in `docs/re/sweep-and-loft.md`
//! ("Loft"); the faces and the matching of points are built by [`anvil_brep::loft`].

use crate::document::*;
use crate::exact;
use crate::rebuild::BuildState;
use anvil_brep as brep;
use anvil_brep::loft::Section;
use serde::{Deserialize, Serialize};

/// A lofted boss or cut through profile sketches, in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoftFeature {
    pub profiles: Vec<u32>,
    pub op: BodyOp,
}

impl Document {
    /// One loft section from a sketch: its one closed loop, or its one free point.
    fn loft_section(&self, id: u32, st: &BuildState) -> Result<Section, String> {
        let sf = self.sketch(id).ok_or("a profile's sketch is missing")?;
        let f = *st.frames.get(&id).ok_or("a profile's sketch failed or is suppressed")?;
        match sf.sketch.profiles() {
            Ok(p) => match p.as_slice() {
                [one] if one.holes.is_empty() => {
                    Ok(Section::Loop { frame: exact::frame(&f), outline: exact::exact_loop(&one.outer) })
                }
                _ => Err("each loft profile must be one closed loop without holes".into()),
            },
            Err(_) => match crate::hole::positions(&sf.sketch).as_slice() {
                [p] => {
                    let w = f.to_world(*p);
                    Ok(Section::Point(glam::DVec3::new(w.x, w.y, w.z)))
                }
                _ => Err("each loft profile must be one closed loop or one point".into()),
            },
        }
    }

    pub(crate) fn rebuild_loft(&self, id: u32, lf: &LoftFeature, st: &mut BuildState) -> Result<(), String> {
        let [a, b] = lf.profiles.as_slice() else {
            return Err(if lf.profiles.len() < 2 {
                "a loft needs two profiles".into()
            } else {
                "lofts through more than two profiles are not available yet".into()
            });
        };
        let (a, b) = (self.loft_section(*a, st)?, self.loft_section(*b, st)?);
        let mut solid = brep::loft::loft_ruled(&a, &b, [None, None]).map_err(|e| e.to_string())?;
        solid.set_owner(id);
        // The faceted tool is the exact one tessellated, so it is the same shape.
        let tool = exact::faceted(std::slice::from_ref(&solid))?;
        let op = lf.op;
        let done = st.exact_step(id, |bodies| exact::combine(bodies, &solid, op));
        if done {
            st.exact_tools.insert(id, (vec![solid], op));
        } else {
            crate::rebuild::apply(&mut st.body, &tool, op)?;
        }
        st.tools.insert(id, (tool, op));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Loft statements 20 and 22 of `docs/re/sweep-and-loft.md`.
    use super::*;
    use anvil_kernel::v2;

    fn square(doc: &mut Document, plane: PlaneRef, half: f64) -> u32 {
        let mut sf = SketchFeature::new(plane);
        sf.sketch.add_rectangle(v2(-half, -half), v2(half, half));
        doc.add_feature(FeatureKind::Sketch(sf))
    }

    #[test]
    fn frustum_and_pyramid() {
        let mut doc = Document::new();
        let lo = square(&mut doc, PlaneRef::Front, 10.0);
        let up = doc
            .add_feature(FeatureKind::Plane(PlaneFeature { offset: Dim::Value(30.0), ..Default::default() }));
        let hi = square(&mut doc, PlaneRef::Feature(up), 5.0);
        let l = doc.add_feature(FeatureKind::Loft(LoftFeature { profiles: vec![lo, hi], op: BodyOp::Boss }));
        assert_eq!(doc.feature(l).map(|f| f.name.as_str()), Some("Loft1"));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!((st.body.mass_properties().volume - 7000.0).abs() < 1e-6);
        assert!(st.exact.is_some());

        let mut doc = Document::new();
        let lo = square(&mut doc, PlaneRef::Front, 10.0);
        let up = doc
            .add_feature(FeatureKind::Plane(PlaneFeature { offset: Dim::Value(30.0), ..Default::default() }));
        let mut sf = SketchFeature::new(PlaneRef::Feature(up));
        sf.sketch.add_point(v2(0.0, 0.0));
        let tip = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Loft(LoftFeature { profiles: vec![lo, tip], op: BodyOp::Boss }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!((st.body.mass_properties().volume - 4000.0).abs() < 1e-6);
    }
}
