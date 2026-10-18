//! Section views: the part shown cut open by one to three planes. A section is a way of
//! looking at the part, saved with it but not a feature: it never changes the part's
//! geometry or mass. The behaviour is specified in `docs/re/measure-and-section.md`
//! ("Section View"): a point is hidden when it lies on the removed side of every active
//! plane, and the cut faces are real faces, so they can be measured.

use crate::document::*;
use crate::exact;
use crate::rebuild::BuildState;
use anvil_brep::boolean::{boolean, BoolOp};
use anvil_kernel::{v2, Frame, Loop2, Profile, Solid, Vec3};
use serde::{Deserialize, Serialize};

/// One section plane: a plane or planar face, turned about its own x axis and then its
/// turned y axis (degrees, about the part's centre projected onto it), then moved along
/// its normal. The side its normal points to is removed, unless reversed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionPlane {
    pub plane: PlaneRef,
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub rotate_x: f64,
    #[serde(default)]
    pub rotate_y: f64,
    #[serde(default)]
    pub reverse: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SectionView {
    pub planes: Vec<SectionPlane>,
    /// Shown; off keeps the planes for next time.
    #[serde(default)]
    pub on: bool,
}

fn turn(v: Vec3, k: Vec3, a: f64) -> Vec3 {
    let (s, c) = a.sin_cos();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

impl Document {
    /// The frames of the section planes, normals pointing into the removed side.
    fn section_frames(&self, view: &SectionView, st: &BuildState) -> Result<Vec<Frame>, String> {
        if view.planes.is_empty() || view.planes.len() > 3 {
            return Err("a section has one to three planes".into());
        }
        let centre = st.body.bounds().center();
        view.planes
            .iter()
            .map(|sp| {
                let f = self.resolve_plane(&sp.plane, st)?;
                let pivot = centre - f.z * (centre - f.origin).dot(f.z);
                let (ax, ay) = (sp.rotate_x.to_radians(), sp.rotate_y.to_radians());
                let (x, y, z) = (f.x, turn(f.y, f.x, ax), turn(f.z, f.x, ax));
                let (x, z) = (turn(x, y, ay), turn(z, y, ay));
                let mut g = Frame { origin: pivot + z * sp.offset, x, y, z };
                if sp.reverse {
                    g = Frame { x: -g.x, z: -g.z, ..g };
                }
                Ok(g)
            })
            .collect()
    }

    /// The part as the section shows it: the faceted body, with what is on the removed
    /// side of every plane taken away.
    pub fn section_body(&self, view: &SectionView, st: &BuildState) -> Result<Solid, String> {
        if st.body.is_empty() {
            return Ok(Solid::default());
        }
        let frames = self.section_frames(view, st)?;
        let b = st.body.bounds();
        let reach = b.diagonal() + b.center().length() + 10.0;
        let sq = Profile::new(
            Loop2::rectangle(v2(-2.0 * reach, -2.0 * reach), v2(2.0 * reach, 2.0 * reach), 0),
            vec![],
        );
        // The removed region: the half-spaces' common part, each a big box from its plane.
        let boxes = |f: &Frame| {
            let o = f.origin + (b.center() - f.origin) - f.z * (b.center() - f.origin).dot(f.z);
            Frame { origin: o, ..*f }
        };
        if let Some(bodies) = &st.exact {
            let exact_cut = || -> Result<Solid, String> {
                let mut removed: Option<Vec<anvil_brep::Solid>> = None;
                for f in &frames {
                    let half = exact::extrude(std::slice::from_ref(&sq), &boxes(f), 0.0, 4.0 * reach, 0)?;
                    removed = Some(match removed {
                        None => half,
                        Some(r) => {
                            let mut next = Vec::new();
                            for a in &r {
                                for h in &half {
                                    next.extend(boolean(a, h, BoolOp::Intersect).map_err(|e| e.to_string())?);
                                }
                            }
                            next
                        }
                    });
                }
                let mut kept = bodies.clone();
                for r in removed.iter().flatten() {
                    kept = exact::combine(&kept, r, BodyOp::Cut)?;
                }
                exact::faceted(&kept)
            };
            if let Ok(s) = exact_cut() {
                return Ok(s);
            }
        }
        let mut removed: Option<Solid> = None;
        for f in &frames {
            let half =
                anvil_kernel::build::extrude(std::slice::from_ref(&sq), &boxes(f), 0.0, 4.0 * reach, 0)
                    .map_err(|e| e.to_string())?;
            removed = Some(match removed {
                None => half,
                Some(r) => r.intersect(&half),
            });
        }
        Ok(match removed {
            Some(r) => st.body.subtract(&r),
            None => st.body.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    //! Section statements 26 to 30 and 33 of `docs/re/measure-and-section.md`.
    use super::*;

    fn block(a: (f64, f64), b: (f64, f64), z: (f64, f64)) -> Document {
        let mut doc = Document::new();
        let plane = doc
            .add_feature(FeatureKind::Plane(PlaneFeature { offset: Dim::Value(z.0), ..Default::default() }));
        let mut sf = SketchFeature::new(PlaneRef::Feature(plane));
        sf.sketch.add_rectangle(v2(a.0, a.1), v2(b.0, b.1));
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: s,
            depth: Dim::Value(z.1 - z.0),
            ..Default::default()
        }));
        doc
    }

    fn cut(doc: &mut Document, planes: Vec<SectionPlane>) -> f64 {
        let st = doc.rebuild();
        let before = st.body.mass_properties().volume;
        let v = doc.section_body(&SectionView { planes, on: true }, &st).unwrap().mass_properties().volume;
        assert_eq!(doc.rebuild().body.mass_properties().volume, before, "sections never change the part");
        v
    }

    fn plane(p: PlaneRef, offset: f64, reverse: bool) -> SectionPlane {
        SectionPlane { plane: p, offset, rotate_x: 0.0, rotate_y: 0.0, reverse }
    }

    #[test]
    fn s26_s27_one_plane_through_the_middle() {
        let mut doc = block((-50.0, -25.0), (50.0, 25.0), (-10.0, 10.0));
        assert!((cut(&mut doc, vec![plane(PlaneRef::Right, 0.0, false)]) - 50000.0).abs() < 1e-6);
        assert!((cut(&mut doc, vec![plane(PlaneRef::Right, 0.0, true)]) - 50000.0).abs() < 1e-6);
        let tilted = SectionPlane { rotate_x: 90.0, ..plane(PlaneRef::Front, 0.0, false) };
        assert!((cut(&mut doc, vec![tilted]) - 50000.0).abs() < 1e-6);
    }

    #[test]
    fn s28_s29_s30_offset_planes_together() {
        let mut doc = block((0.0, 0.0), (100.0, 50.0), (0.0, 20.0));
        // The Right plane offset to x = 40: the pivot is the box centre's projection,
        // so the offset is measured from x = 0.
        let x40 = plane(PlaneRef::Right, 40.0, false);
        assert!((cut(&mut doc, vec![x40.clone()]) - 40000.0).abs() < 1e-6);
        assert!((cut(&mut doc, vec![SectionPlane { reverse: true, ..x40.clone() }]) - 60000.0).abs() < 1e-6);
        let y25 = plane(PlaneRef::Top, 25.0, false);
        assert!((cut(&mut doc, vec![x40.clone(), y25.clone()]) - 70000.0).abs() < 1e-6);
        let z10 = plane(PlaneRef::Front, 10.0, false);
        assert!((cut(&mut doc, vec![x40, y25, z10]) - 85000.0).abs() < 1e-6);
    }
}
