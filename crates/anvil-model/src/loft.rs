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
        if lf.profiles.len() < 2 {
            return Err("a loft needs two profiles".into());
        }
        if lf.profiles.len() > 2 {
            let sections =
                lf.profiles.iter().map(|p| self.loft_section(*p, st)).collect::<Result<Vec<_>, _>>()?;
            let tool = faceted_skin(&sections, id)?;
            st.leave_exact_for(id, "lofts through more than two profiles are built on the faceted kernel");
            crate::rebuild::apply(&mut st.body, &tool, lf.op)?;
            st.tools.insert(id, (tool, lf.op));
            return Ok(());
        }
        let [a, b] = lf.profiles.as_slice() else {
            return Err("a loft needs two profiles".into());
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

/// A smooth skin through three or more sections, on the faceted kernel. Every section is
/// lined up with the first (start point and direction, as for a ruled loft) and sampled
/// at the same fractions of its length; each run of matching points is joined by a
/// Catmull-Rom curve through the sections, so equal sections give a straight prism.
fn faceted_skin(sections: &[Section], id: u32) -> Result<anvil_kernel::Solid, String> {
    use anvil_kernel::{FaceId, FaceTag, Polygon, SurfaceKind, Vec3};
    let n = sections.len();
    if sections[1..n - 1].iter().any(|s| matches!(s, Section::Point(_))) {
        return Err("only the first or last profile of a loft may be a point".into());
    }
    let m = 128;
    let e = |k: anvil_brep::KernelError| k.to_string();
    // Each loop lined up with its neighbour and sampled at m points by length.
    let mut rings: Vec<Vec<glam::DVec3>> = Vec::with_capacity(n);
    let mut prev: Option<Section> = None;
    for (k, s) in sections.iter().enumerate() {
        let neighbour = if k == 0 { &sections[1] } else { prev.as_ref().unwrap_or(&sections[k - 1]) };
        let a = if k == 0 {
            let [x, _] = brep::loft::match_sections(s, neighbour, [None, None]).map_err(e)?;
            x
        } else {
            let [_, y] = brep::loft::match_sections(neighbour, s, [None, None]).map_err(e)?;
            y
        };
        rings.push(sample(&a, m));
        prev = Some(a);
    }
    let ring_at = |k: usize, j: usize| rings[k][j % rings[k].len()];
    // Catmull-Rom between sections, 16 steps a span.
    let steps = 16;
    let mut stations: Vec<Vec<glam::DVec3>> = Vec::new();
    for k in 0..n - 1 {
        for t in 0..steps {
            let u = t as f64 / steps as f64;
            stations.push(
                (0..m)
                    .map(|j| {
                        let p1 = ring_at(k, j);
                        let p2 = ring_at(k + 1, j);
                        let p0 = if k == 0 { p1 * 2.0 - p2 } else { ring_at(k - 1, j) };
                        let p3 = if k + 2 >= n { p2 * 2.0 - p1 } else { ring_at(k + 2, j) };
                        let (u2, u3) = (u * u, u * u * u);
                        (p1 * 2.0
                            + (p2 - p0) * u
                            + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * u2
                            + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * u3)
                            * 0.5
                    })
                    .collect(),
            );
        }
    }
    stations.push(rings[n - 1].clone());
    let v = |p: glam::DVec3| Vec3 { x: p.x, y: p.y, z: p.z };
    let tag = |index: u32, surface| FaceTag { id: FaceId { feature: id, index }, surface };
    let mut polys = Vec::new();
    for w in stations.windows(2) {
        for j in 0..m {
            let (a, b, c, d) = (w[0][j], w[0][(j + 1) % m], w[1][(j + 1) % m], w[1][j]);
            polys.extend(Polygon::new(vec![v(a), v(b), v(c)], tag(2, SurfaceKind::Other)));
            polys.extend(Polygon::new(vec![v(a), v(c), v(d)], tag(2, SurfaceKind::Other)));
        }
    }
    // Caps: fans from each end ring's centre (a point section needs none).
    for (ring, index, flip) in
        [(&stations[0], FaceId::START_CAP, true), (&stations[stations.len() - 1], FaceId::END_CAP, false)]
    {
        let c = ring.iter().fold(glam::DVec3::ZERO, |s, p| s + *p) / m as f64;
        if ring.iter().all(|p| (*p - c).length() < 1e-9) {
            continue;
        }
        for j in 0..m {
            let (a, b) = (ring[j], ring[(j + 1) % m]);
            let tri = if flip { vec![v(c), v(b), v(a)] } else { vec![v(c), v(a), v(b)] };
            polys.extend(Polygon::new(tri, tag(index, SurfaceKind::Plane)));
        }
    }
    let mut solid = anvil_kernel::Solid::new(polys);
    if solid.mass_properties().volume < 0.0 {
        solid.flip();
    }
    Ok(solid)
}

/// `m` points round a lined-up section, evenly by length from its start.
fn sample(s: &Section, m: usize) -> Vec<glam::DVec3> {
    let segs = s.segment_count();
    if segs == 0 {
        return match s {
            Section::Point(p) => vec![*p; m],
            Section::Loop { .. } => Vec::new(),
        };
    }
    let fine: Vec<glam::DVec3> =
        (0..segs).flat_map(|i| (0..64).map(move |k| s.point_at(i, k as f64 / 64.0))).collect();
    let mut cum = vec![0.0];
    for i in 0..fine.len() {
        let next = cum[i] + (fine[(i + 1) % fine.len()] - fine[i]).length();
        cum.push(next);
    }
    let total = cum[fine.len()];
    (0..m)
        .map(|j| {
            let target = total * j as f64 / m as f64;
            let i = cum.partition_point(|c| *c <= target).saturating_sub(1).min(fine.len() - 1);
            let span = (cum[i + 1] - cum[i]).max(1e-300);
            let t = (target - cum[i]) / span;
            fine[i] + (fine[(i + 1) % fine.len()] - fine[i]) * t
        })
        .collect()
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
    fn three_equal_circles_make_a_cylinder() {
        // Statement 25: no bulge between equal sections.
        let mut doc = Document::new();
        let mut ids = Vec::new();
        for z in [0.0, 20.0, 40.0] {
            let p = doc.add_feature(FeatureKind::Plane(PlaneFeature {
                offset: Dim::Value(z),
                ..Default::default()
            }));
            let mut sf = SketchFeature::new(PlaneRef::Feature(p));
            let c = sf.sketch.add_point(v2(0.0, 0.0));
            sf.sketch.add_circle(c, 10.0);
            ids.push(doc.add_feature(FeatureKind::Sketch(sf)));
        }
        doc.add_feature(FeatureKind::Loft(LoftFeature { profiles: ids, op: BodyOp::Boss }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let v = st.body.mass_properties().volume;
        assert!((v - 4000.0 * std::f64::consts::PI).abs() < 2e-3 * 4000.0 * std::f64::consts::PI, "{v}");
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
